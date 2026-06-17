#pragma once

#include "BenchCerts.h"

#include <boost/asio/co_spawn.hpp>
#include <boost/asio/detached.hpp>
#include <boost/asio/executor_work_guard.hpp>
#include <boost/asio/io_context.hpp>
#include <boost/asio/ip/tcp.hpp>
#include <boost/asio/post.hpp>
#include <boost/asio/redirect_error.hpp>
#include <boost/asio/socket_base.hpp>
#include <boost/asio/ssl.hpp>
#include <boost/asio/use_awaitable.hpp>
#include <boost/beast/core.hpp>
#include <boost/beast/http.hpp>

#include <cstddef>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace bench {

namespace net = boost::asio;
namespace ssl = boost::asio::ssl;
namespace http = boost::beast::http;
using tcp = boost::asio::ip::tcp;

// A loopback HTTP/1.1 server used as the benchmark target. It honours
// keep-alive (so the Rust client's pooled connections are reused) and closes
// after a single response when the client asks for it (the legacy client always
// sends `Connection: close`). The whole thing — including its TLS certificate —
// is self-contained: no external files, no runtime cert generation.
//
// One instance is created per benchmark run (per "test case"): it binds a fresh
// ephemeral port in the constructor, starts serving immediately on its own
// io_context thread pool, exposes that port via port(), and shuts everything
// down in the destructor. A fresh server per run means a fresh TCP 4-tuple
// space and fresh server threads, so nothing carries over between runs.
class BenchServer
{
public:
    BenchServer(unsigned serverThreads, std::size_t responseSize, bool tls)
        : sslCtx_(ssl::context::tls_server)
        , acceptor_(ioc_)
        , tls_(tls)
        , body_(responseSize, 'x')
    {
        unsigned const threads = serverThreads != 0 ? serverThreads : 1;
        if (tls_)
            configureTls();

        tcp::endpoint ep(net::ip::make_address("127.0.0.1"), 0);
        acceptor_.open(ep.protocol());
        acceptor_.set_option(net::socket_base::reuse_address(true));
        acceptor_.bind(ep);
        acceptor_.listen(net::socket_base::max_listen_connections);
        port_ = acceptor_.local_endpoint().port();

        net::co_spawn(ioc_, acceptLoop(), net::detached);
        work_.emplace(net::make_work_guard(ioc_));
        for (unsigned i = 0; i < threads; ++i)
            pool_.emplace_back([this] { ioc_.run(); });
    }

    BenchServer(BenchServer const&) = delete;
    BenchServer&
    operator=(BenchServer const&) = delete;

    ~BenchServer()
    {
        stop();
    }

    [[nodiscard]] unsigned short
    port() const
    {
        return port_;
    }

private:
    void
    stop()
    {
        if (pool_.empty())
            return;

        net::post(ioc_, [this] {
            boost::system::error_code ec;
            acceptor_.close(ec);
        });
        work_.reset();
        ioc_.stop();
        for (auto& t : pool_)
            if (t.joinable())
                t.join();
        pool_.clear();
    }

    void
    configureTls()
    {
        sslCtx_.use_certificate_chain(
            net::buffer(certs::kServerChainPem.data(), certs::kServerChainPem.size()));
        sslCtx_.use_private_key(
            net::buffer(certs::kServerKeyPem.data(), certs::kServerKeyPem.size()),
            ssl::context::pem);
    }

    net::awaitable<void>
    acceptLoop()
    {
        for (;;)
        {
            boost::system::error_code ec;
            tcp::socket socket =
                co_await acceptor_.async_accept(net::redirect_error(net::use_awaitable, ec));
            if (ec)
            {
                if (ec == net::error::operation_aborted)
                    break;
                continue;
            }
            if (tls_)
                net::co_spawn(ioc_, sessionTls(std::move(socket)), net::detached);
            else
                net::co_spawn(ioc_, sessionPlain(std::move(socket)), net::detached);
        }
    }

    net::awaitable<void>
    sessionPlain(tcp::socket socket)
    {
        try
        {
            co_await serve(socket);
            boost::system::error_code ec;
            socket.shutdown(tcp::socket::shutdown_send, ec);
        }
        catch (std::exception const&)
        {
            // Client closed early / reset — expected at end of a keep-alive run.
        }
    }

    net::awaitable<void>
    sessionTls(tcp::socket rawSocket)
    {
        try
        {
            ssl::stream<tcp::socket> stream(std::move(rawSocket), sslCtx_);
            co_await stream.async_handshake(ssl::stream_base::server, net::use_awaitable);
            co_await serve(stream);
            boost::system::error_code ec;
            co_await stream.async_shutdown(net::redirect_error(net::use_awaitable, ec));
        }
        catch (std::exception const&)
        {
            // Handshake failure or early client close — expected during teardown.
        }
    }

    // Keep-alive loop shared by the plain and TLS paths. Reads a request and
    // replies 200 with a fixed body until the peer signals close.
    template <class Stream>
    net::awaitable<void>
    serve(Stream& stream)
    {
        boost::beast::flat_buffer buffer;
        for (;;)
        {
            http::request<http::string_body> req;
            co_await http::async_read(stream, buffer, req, net::use_awaitable);

            http::response<http::string_body> res{http::status::ok, req.version()};
            res.set(http::field::server, "bench");
            res.set(http::field::content_type, "application/octet-stream");
            res.keep_alive(req.keep_alive());
            res.body() = body_;
            res.prepare_payload();

            co_await http::async_write(stream, res, net::use_awaitable);

            if (!req.keep_alive())
                break;
        }
    }

    net::io_context ioc_;
    ssl::context sslCtx_;
    tcp::acceptor acceptor_;
    bool tls_;
    unsigned short port_{0};
    std::string body_;
    std::vector<std::thread> pool_;
    std::optional<net::executor_work_guard<net::io_context::executor_type>> work_;
};

}  // namespace bench
