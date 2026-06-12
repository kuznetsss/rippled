#include <xrpl/net/HTTPClientRust.h>

// Generated cxx bridge — rs::http_client::init_tokio_runtime, etc.
#include <boost/asio/io_context.hpp>
#include <boost/asio/use_future.hpp>

#include <gtest/gtest.h>
#include <rs_http_client_cxxbridge/ffi.h>

#include <chrono>
#include <future>
#include <string>
#include <thread>
#include <vector>

using namespace xrpl;

// ---------------------------------------------------------------------------
// Test suite fixture
//
// The Tokio runtime is a OnceLock: it can be initialised ONCE per process
// and cannot be re-initialised after shutdown.  We therefore:
//   • Start it once in SetUpTestSuite with a small thread pool.
//   • Do NOT shut it down between individual tests.
//   • Optionally shut it down in TearDownTestSuite.
// ---------------------------------------------------------------------------
class HTTPClientRustTest : public ::testing::Test
{
protected:
    static void
    SetUpTestSuite()
    {
        auto status = ::rs::http_client::init_tokio_runtime(2);
        // AlreadyInitialized is fine if another test suite already called init.
        ASSERT_TRUE(
            status.code == ::rs::http_client::ErrorCode::Ok ||
            status.code == ::rs::http_client::ErrorCode::AlreadyInitialized)
            << static_cast<std::string>(status.message);
    }

    static void
    TearDownTestSuite()
    {
        // Best-effort shutdown; ignore errors (e.g. already-shut-down).
        ::rs::http_client::shutdown_tokio_runtime(2000 /*ms*/);
    }

    /** Build a minimal GET request to @p url with a 5 s timeout. */
    static ::rs::http_client::Request
    makeRequest(std::string const& url)
    {
        ::rs::http_client::Request req;
        req.method = ::rs::http_client::HTTPMethod::Get;
        // rust::String is assigned from std::string by copy, so no std::move.
        req.url = url;
        req.timeout_ms = 5000;
        req.max_response_bytes = 1024 * 1024;
        return req;
    }
};

// ---------------------------------------------------------------------------
// Helper: run the io_context until the handler fires or a timeout elapses.
// ---------------------------------------------------------------------------
static bool
runUntilDone(boost::asio::io_context& ioc, bool& done, int timeoutMs = 5000)
{
    auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeoutMs);
    while (!done)
    {
        if (ioc.run_one() == 0)
            break;
        if (std::chrono::steady_clock::now() > deadline)
            break;
    }
    return done;
}

// ---------------------------------------------------------------------------
// Core test: basic round-trip via a lambda token.
//
// The stub returns status=200, header x-stub:true, and a body that starts
// with "stub response for".
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, BasicRequest)
{
    boost::asio::io_context ioc;
    bool done = false;
    boost::system::error_code resultEc;
    ::rs::http_client::Response resultResp;

    HTTPClientRust::asyncRequest(
        ioc.get_executor(),
        makeRequest("http://example.com/test"),
        [&](boost::system::error_code ec, ::rs::http_client::Response resp) {
            resultEc = ec;
            resultResp = std::move(resp);
            done = true;
        });

    ASSERT_TRUE(runUntilDone(ioc, done)) << "handler did not fire in time";

    EXPECT_FALSE(resultEc) << resultEc.message();
    EXPECT_EQ(resultResp.status, 200);

    // Body should contain "stub response for"
    std::string body(reinterpret_cast<char const*>(resultResp.body.data()), resultResp.body.size());
    EXPECT_NE(body.find("stub response for"), std::string::npos) << "body: " << body;

    // x-stub header should be present
    bool stubHeaderFound = false;
    for (auto const& h : resultResp.headers)
    {
        if (static_cast<std::string>(h.name) == "x-stub")
        {
            stubHeaderFound = true;
            EXPECT_EQ(static_cast<std::string>(h.value), "true");
        }
    }
    EXPECT_TRUE(stubHeaderFound) << "x-stub header not found";
}

// ---------------------------------------------------------------------------
// Executor-affinity test: the handler must fire on the io_context thread.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, HandlerOnIocThread)
{
    boost::asio::io_context ioc;
    bool done = false;
    std::thread::id handlerThreadId;
    std::thread::id iocThreadId;

    HTTPClientRust::asyncRequest(
        ioc.get_executor(),
        makeRequest("http://example.com/thread-test"),
        [&](boost::system::error_code /*ec*/, ::rs::http_client::Response /*r*/) {
            handlerThreadId = std::this_thread::get_id();
            done = true;
        });

    // Run the io_context on *this* thread and capture its thread id.
    iocThreadId = std::this_thread::get_id();
    ASSERT_TRUE(runUntilDone(ioc, done)) << "handler did not fire in time";

    EXPECT_EQ(handlerThreadId, iocThreadId)
        << "handler was not dispatched onto the io_context thread";
}

// ---------------------------------------------------------------------------
// use_future test: proves async_initiate genericity.
//
// boost::asio::use_future with signature void(error_code, Response) returns a
// std::future<Response>: Boost automatically promotes a non-zero error_code to
// a boost::system::system_error exception and discards the error_code arg.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, UseFuture)
{
    boost::asio::io_context ioc;

    auto fut = HTTPClientRust::asyncRequest(
        ioc.get_executor(), makeRequest("http://example.com/future-test"), boost::asio::use_future);

    // Run the io_context in a background thread so ioc.run() drives the op
    // while we wait on the future in the main thread.
    std::thread runner([&] { ioc.run(); });

    ::rs::http_client::Response result;
    ASSERT_NO_THROW(result = fut.get());

    runner.join();

    EXPECT_EQ(result.status, 200);
}

// ---------------------------------------------------------------------------
// Failover test: asyncRequestAny with multiple "sites".
//
// The stub succeeds for every URL, so the call completes on the first entry.
// ---------------------------------------------------------------------------
TEST_F(HTTPClientRustTest, FailoverAllSucceed)
{
    boost::asio::io_context ioc;
    bool done = false;
    boost::system::error_code resultEc;
    ::rs::http_client::Response resultResp;

    std::vector<::rs::http_client::Request> reqs;
    reqs.push_back(makeRequest("http://site1.example.com/"));
    reqs.push_back(makeRequest("http://site2.example.com/"));
    reqs.push_back(makeRequest("http://site3.example.com/"));

    HTTPClientRust::asyncRequestAny(
        ioc.get_executor(),
        std::move(reqs),
        [&](boost::system::error_code ec, ::rs::http_client::Response resp) {
            resultEc = ec;
            resultResp = std::move(resp);
            done = true;
        });

    ASSERT_TRUE(runUntilDone(ioc, done)) << "handler did not fire in time";

    EXPECT_FALSE(resultEc) << resultEc.message();
    EXPECT_EQ(resultResp.status, 200);

    std::string body(reinterpret_cast<char const*>(resultResp.body.data()), resultResp.body.size());
    EXPECT_NE(body.find("stub response for"), std::string::npos) << "body: " << body;
}
