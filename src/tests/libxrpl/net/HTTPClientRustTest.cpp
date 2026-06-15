#include <xrpl/net/HTTPClientRust.h>

#include <boost/asio/awaitable.hpp>
#include <boost/asio/co_spawn.hpp>  // IWYU pragma: keep
#include <boost/asio/detached.hpp>
#include <boost/asio/io_context.hpp>
#include <boost/asio/ip/tcp.hpp>
#include <boost/asio/post.hpp>
#include <boost/asio/socket_base.hpp>
#include <boost/asio/steady_timer.hpp>
#include <boost/asio/use_awaitable.hpp>
#include <boost/asio/use_future.hpp>
#include <boost/beast/core.hpp>  // IWYU pragma: keep
#include <boost/beast/http.hpp>  // IWYU pragma: keep

#include <gtest/gtest.h>
#include <rs_http_client_cxxbridge/ffi.h>

#include <chrono>
#include <cstdint>
#include <exception>
#include <string>
#include <thread>
#include <vector>

using namespace xrpl;

namespace {

// Minimal HTTP/1.1 server over loopback, mirroring the Beast-based server in
// the sibling HTTPClient.cpp test.  It accepts one connection at a time,
// records the request method/target/body, and replies with a canned status and
// body.  The actual client socket I/O happens on Tokio worker threads inside
// the Rust crate; this server only needs its own io_context pumped so the
// accept/serve coroutine makes progress.
class TestHTTPServer
{
private:
    boost::asio::io_context ioc_;
    boost::asio::ip::tcp::acceptor acceptor_;
    bool running_{true};
    bool finished_{false};
    unsigned short port_{0};

    std::string responseBody_;
    unsigned int statusCode_{200};

    // Captured from the most recent request.
    std::string lastBody_;
    std::string lastTarget_;
    std::string lastMethod_;

public:
    TestHTTPServer() : acceptor_(ioc_)
    {
        boost::asio::ip::tcp::endpoint endpoint{boost::asio::ip::tcp::v4(), 0};
        acceptor_.open(endpoint.protocol());
        acceptor_.set_option(boost::asio::socket_base::reuse_address(true));
        acceptor_.bind(endpoint);
        acceptor_.listen();
        port_ = acceptor_.local_endpoint().port();

        boost::asio::co_spawn(ioc_, accept(), boost::asio::detached);
    }

    TestHTTPServer(TestHTTPServer&&) = delete;
    TestHTTPServer&
    operator=(TestHTTPServer&&) = delete;

    ~TestHTTPServer() = default;

    boost::asio::io_context&
    ioc()
    {
        return ioc_;
    }

    [[nodiscard]] unsigned short
    port() const
    {
        return port_;
    }

    void
    setResponseBody(std::string body)
    {
        responseBody_ = std::move(body);
    }

    void
    setStatusCode(unsigned int code)
    {
        statusCode_ = code;
    }

    [[nodiscard]] std::string const&
    lastBody() const
    {
        return lastBody_;
    }

    [[nodiscard]] std::string const&
    lastMethod() const
    {
        return lastMethod_;
    }

    [[nodiscard]] std::string const&
    lastTarget() const
    {
        return lastTarget_;
    }

    // Close the acceptor so the accept() coroutine unwinds.  Must run on the
    // server's io_context thread (post it from elsewhere).
    void
    stop()
    {
        running_ = false;
        acceptor_.close();
    }

    [[nodiscard]] bool
    finished() const
    {
        return finished_;
    }

private:
    boost::asio::awaitable<void>
    accept()
    {
        while (running_)
        {
            try
            {
                auto socket = co_await acceptor_.async_accept(boost::asio::use_awaitable);
                if (!running_)
                    break;
                co_await handleConnection(std::move(socket));
            }
            catch (std::exception const&)
            {
                break;
            }
        }
        finished_ = true;
    }

    // Exceptions (e.g. a client that closes early) propagate to accept()'s
    // handler, which ends the serve loop — acceptable since each test issues a
    // single request.
    boost::asio::awaitable<void>
    handleConnection(boost::asio::ip::tcp::socket socket)
    {
        boost::beast::flat_buffer buffer;
        boost::beast::http::request<boost::beast::http::string_body> req;
        co_await boost::beast::http::async_read(socket, buffer, req, boost::asio::use_awaitable);

        lastBody_ = req.body();
        lastTarget_ = req.target();
        lastMethod_ = std::string{req.method_string()};

        boost::beast::http::response<boost::beast::http::string_body> res;
        res.version(req.version());
        res.result(statusCode_);
        res.set(boost::beast::http::field::server, "TestServer");
        res.body() = responseBody_;
        res.prepare_payload();

        co_await boost::beast::http::async_write(socket, res, boost::asio::use_awaitable);

        boost::system::error_code shutdownEc;
        // NOLINTNEXTLINE(bugprone-unused-return-value)
        socket.shutdown(boost::asio::ip::tcp::socket::shutdown_send, shutdownEc);
    }
};

}  // namespace

// ---------------------------------------------------------------------------
// Fixture.
//
// The Tokio runtime is a process-wide OnceLock: initialise it once for the
// whole suite.  The reqwest client (TLS context) is rebuilt before each test
// with verification disabled, which is sufficient for plain-HTTP loopback.
// ---------------------------------------------------------------------------
class HTTPClientRustTest : public ::testing::Test
{
protected:
    static void
    SetUpTestSuite()
    {
        auto status = ::rs::http_client::init_tokio_runtime(2);
        ASSERT_TRUE(
            status.code == ::rs::http_client::ErrorCode::Ok ||
            status.code == ::rs::http_client::ErrorCode::AlreadyInitialized)
            << static_cast<std::string>(status.message);
    }

    static void
    TearDownTestSuite()
    {
        ::rs::http_client::shutdown_tokio_runtime(2000 /*ms*/);
    }

    void
    SetUp() override
    {
        ::rs::http_client::TlsConfig cfg{};
        cfg.verify = false;
        auto status = ::rs::http_client::init_tls_context(cfg);
        ASSERT_EQ(status.code, ::rs::http_client::ErrorCode::Ok)
            << static_cast<std::string>(status.message);
    }

    void
    TearDown() override
    {
        ::rs::http_client::reset_tls_context();
    }

    static std::string
    url(TestHTTPServer const& server, std::string const& path)
    {
        return "http://127.0.0.1:" + std::to_string(server.port()) + path;
    }
};

// Drive `ioc` on the current thread.  The completion handler is expected to
// call ioc.stop() when it fires, which makes run() return promptly; the
// deadline timer is only a safety net that bounds a hung test.
static void
runWithDeadline(boost::asio::io_context& ioc, std::chrono::seconds timeout = std::chrono::seconds(15))
{
    boost::asio::steady_timer deadline(ioc);
    deadline.expires_after(timeout);
    deadline.async_wait([&ioc](boost::system::error_code const& ec) {
        if (ec != boost::asio::error::operation_aborted)
            ioc.stop();
    });
    ioc.run();
    deadline.cancel();
}

// ---------------------------------------------------------------------------
// Round-trip: a POST whose body the server echoes back to us.  Validates the
// request-body marshaling fix (server receives the exact bytes) and the
// response-body path (body returned intact).
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, PostBodyRoundTrip)
{
    TestHTTPServer server;
    std::string const responseBody = "response payload from server";
    server.setResponseBody(responseBody);

    std::string const requestBody = "request body bytes \x01\x02\x03 end";
    std::vector<uint8_t> bodyBytes(requestBody.begin(), requestBody.end());

    bool done = false;
    boost::system::error_code resultEc;
    ::rs::http_client::Response resultResp;

    HTTPRequestBuilder(
        url(server, "/echo"), ::rs::http_client::HTTPMethod::Post, std::chrono::seconds(5))
        .addHeader("content-type", "application/octet-stream")
        .setBody(std::move(bodyBytes))
        .asyncSubmit(
            server.ioc().get_executor(),
            [&](boost::system::error_code ec, ::rs::http_client::Response resp) {
                resultEc = ec;
                resultResp = std::move(resp);
                done = true;
                server.ioc().stop();
            });

    runWithDeadline(server.ioc());

    ASSERT_TRUE(done) << "handler did not fire in time";
    EXPECT_FALSE(resultEc) << resultEc.message();
    EXPECT_EQ(resultResp.status, 200);

    EXPECT_EQ(server.lastMethod(), "POST");
    EXPECT_EQ(server.lastTarget(), "/echo");
    EXPECT_EQ(server.lastBody(), requestBody);

    std::string const got(
        reinterpret_cast<char const*>(resultResp.body.data()), resultResp.body.size());
    EXPECT_EQ(got, responseBody);
}

// ---------------------------------------------------------------------------
// A larger response body to exercise the Content-Length-based reservation and
// chunked accumulation path without truncation.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, LargeResponseBody)
{
    TestHTTPServer server;
    std::string const responseBody(256 * 1024, 'x');
    server.setResponseBody(responseBody);

    bool done = false;
    boost::system::error_code resultEc;
    ::rs::http_client::Response resultResp;

    HTTPRequestBuilder(
        url(server, "/large"), ::rs::http_client::HTTPMethod::Get, std::chrono::seconds(5))
        .asyncSubmit(
            server.ioc().get_executor(),
            [&](boost::system::error_code ec, ::rs::http_client::Response resp) {
                resultEc = ec;
                resultResp = std::move(resp);
                done = true;
                server.ioc().stop();
            });

    runWithDeadline(server.ioc());

    ASSERT_TRUE(done) << "handler did not fire in time";
    EXPECT_FALSE(resultEc) << resultEc.message();
    EXPECT_EQ(resultResp.status, 200);
    EXPECT_EQ(resultResp.body.size(), responseBody.size());
}

// ---------------------------------------------------------------------------
// The completion handler must be dispatched on the io_context thread, not a
// Tokio worker thread.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, HandlerOnIocThread)
{
    TestHTTPServer server;
    server.setResponseBody("ok");

    bool done = false;
    std::thread::id handlerThreadId;

    HTTPRequestBuilder(
        url(server, "/thread"), ::rs::http_client::HTTPMethod::Get, std::chrono::seconds(5))
        .asyncSubmit(
            server.ioc().get_executor(),
            [&](boost::system::error_code /*ec*/, ::rs::http_client::Response /*resp*/) {
                handlerThreadId = std::this_thread::get_id();
                done = true;
                server.ioc().stop();
            });

    auto const iocThreadId = std::this_thread::get_id();
    runWithDeadline(server.ioc());

    ASSERT_TRUE(done) << "handler did not fire in time";
    EXPECT_EQ(handlerThreadId, iocThreadId)
        << "handler was not dispatched onto the io_context thread";
}

// ---------------------------------------------------------------------------
// use_future token: proves async_initiate genericity with the corrected
// two-argument completion signature.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, UseFuture)
{
    TestHTTPServer server;
    server.setResponseBody("future body");

    auto fut = HTTPRequestBuilder(
                   url(server, "/future"),
                   ::rs::http_client::HTTPMethod::Get,
                   std::chrono::seconds(5))
                   .asyncSubmit(server.ioc().get_executor(), boost::asio::use_future);

    std::thread runner([&] { server.ioc().run(); });

    ::rs::http_client::Response result;
    ASSERT_NO_THROW(result = fut.get());

    // Stop the acceptor on the server's own thread, then let run() drain.
    boost::asio::post(server.ioc(), [&server] { server.stop(); });
    runner.join();

    EXPECT_EQ(result.status, 200);
    std::string const got(
        reinterpret_cast<char const*>(result.body.data()), result.body.size());
    EXPECT_EQ(got, "future body");
}

// ---------------------------------------------------------------------------
// Error path: with no TLS context (reqwest client) initialised, a request must
// complete with NotInitialized -> operation_not_permitted, delivered on the
// io_context thread.  No network is involved.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, NotInitializedSurfacesError)
{
    // SetUp() built a client; drop it so the request short-circuits.
    ::rs::http_client::reset_tls_context();

    boost::asio::io_context ioc;
    bool done = false;
    boost::system::error_code resultEc;

    HTTPRequestBuilder(
        "http://127.0.0.1:1/never", ::rs::http_client::HTTPMethod::Get, std::chrono::seconds(5))
        .asyncSubmit(
            ioc.get_executor(),
            [&](boost::system::error_code ec, ::rs::http_client::Response /*resp*/) {
                resultEc = ec;
                done = true;
                ioc.stop();
            });

    runWithDeadline(ioc, std::chrono::seconds(5));

    ASSERT_TRUE(done) << "handler did not fire in time";
    EXPECT_EQ(resultEc, boost::system::errc::operation_not_permitted);
}
