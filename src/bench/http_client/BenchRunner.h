#pragma once

#include <xrpl/beast/utility/Journal.h>
#include <xrpl/net/HTTPClient.h>
#include <xrpl/net/HTTPClientRust.h>
#include <rs_http_client_cxxbridge/ffi.h>

#include <boost/asio/io_context.hpp>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <expected>
#include <functional>
#include <mutex>
#include <numeric>
#include <string>
#include <sys/resource.h>
#include <thread>
#include <vector>

namespace bench {

enum class ClientKind { Legacy, Rust };

struct RunConfig
{
    ClientKind client;
    bool tls;                      // true => https, false => http
    std::string host;              // "127.0.0.1"
    unsigned short port;           // server port for the chosen transport
    std::string path;              // "/bench"
    std::size_t maxResponseBytes;  // response-size cap passed to both clients
    unsigned totalRequests;        // measured requests
    unsigned warmupRequests;       // unmeasured priming requests
    unsigned concurrency;          // number of in-flight requests held constant
    unsigned clientThreads;        // asio io_context threads driving the client
    std::chrono::milliseconds timeout;
    // When true (the Rust forced-close cell), send `Connection: close` so the
    // SERVER initiates the TCP close — mirroring the legacy client and keeping
    // the active-close (and thus TIME_WAIT) on the server's fixed port rather
    // than exhausting the client's ephemeral ports on loopback. Ignored by the
    // legacy path, which always sends `Connection: close`.
    bool requestConnectionClose = false;
};

struct RunResult
{
    unsigned ok = 0;
    unsigned errors = 0;
    double wallSeconds = 0;        // measured window only
    double throughputRps = 0;      // (ok+errors)/wallSeconds
    double p50Ms = 0, p90Ms = 0, p99Ms = 0, maxMs = 0, meanMs = 0;
    double cpuUserSeconds = 0;     // getrusage(RUSAGE_SELF) delta over measured window
    double cpuSysSeconds = 0;
    long peakRssBytes = 0;         // ru_maxrss, normalized to bytes
    std::string firstError;        // first error message seen (diagnostics)
};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

namespace detail {

// Compute percentile p (0..100) from a sorted latency vector.
// Uses: index = clamp(ceil(p/100 * N) - 1, 0, N-1).
inline double
percentile(std::vector<double> const& sorted, double p)
{
    std::size_t const n = sorted.size();
    if (n == 0)
        return 0.0;
    auto idx = static_cast<std::ptrdiff_t>(std::ceil(p / 100.0 * static_cast<double>(n)) - 1);
    idx = std::clamp(idx, std::ptrdiff_t{0}, static_cast<std::ptrdiff_t>(n - 1));
    return sorted[static_cast<std::size_t>(idx)];
}

inline void
fillStats(RunResult& r, std::vector<double>& latencies)
{
    if (!latencies.empty())
    {
        std::sort(latencies.begin(), latencies.end());
        r.p50Ms = percentile(latencies, 50);
        r.p90Ms = percentile(latencies, 90);
        r.p99Ms = percentile(latencies, 99);
        r.maxMs = latencies.back();
        r.meanMs = std::accumulate(latencies.begin(), latencies.end(), 0.0) /
            static_cast<double>(latencies.size());
    }

    if (r.wallSeconds > 0.0)
        r.throughputRps = static_cast<double>(r.ok + r.errors) / r.wallSeconds;
}

inline double
timevalToSeconds(struct timeval const& tv)
{
    return static_cast<double>(tv.tv_sec) + static_cast<double>(tv.tv_usec) / 1.0e6;
}

// Run one closed-loop phase of `count` requests at `concurrency` on a fresh
// io_context with `clientThreads` threads, using the legacy HTTPClient::get.
// Latencies are appended to `latenciesOut` when non-null (nullptr => warmup).
inline void
runLegacyPhase(
    RunConfig const& cfg,
    unsigned count,
    std::vector<double>* latenciesOut,
    unsigned& okOut,
    unsigned& errorsOut,
    std::string& firstErrorOut)
{
    boost::asio::io_context ioc;

    std::atomic<unsigned> launched{0};
    std::atomic<unsigned> completed{0};
    std::mutex mu;

    auto const timeoutSec =
        std::chrono::ceil<std::chrono::seconds>(cfg.timeout) < std::chrono::seconds(1)
        ? std::chrono::seconds(1)
        : std::chrono::ceil<std::chrono::seconds>(cfg.timeout);

    // Forward-declare so the lambda can reference itself.
    std::function<void()> fireOne;
    fireOne = [&]()
    {
        unsigned const slot = launched.fetch_add(1, std::memory_order_relaxed);
        if (slot >= count)
        {
            // Over-guard: we incremented past the limit — undo and stop.
            launched.fetch_sub(1, std::memory_order_relaxed);
            return;
        }

        auto const start = std::chrono::steady_clock::now();

        xrpl::HTTPClient::get(
            cfg.tls,
            ioc,
            cfg.host,
            cfg.port,
            cfg.path,
            cfg.maxResponseBytes,
            timeoutSec,
            [&, start](
                boost::system::error_code const& ec,
                int status,
                std::string const& /*data*/) -> bool
            {
                double const ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - start).count();

                {
                    std::lock_guard<std::mutex> lk(mu);
                    if (!ec && status == 200)
                    {
                        ++okOut;
                    }
                    else
                    {
                        ++errorsOut;
                        if (firstErrorOut.empty())
                        {
                            if (ec)
                                firstErrorOut = ec.message();
                            else
                                firstErrorOut = "status " + std::to_string(status);
                        }
                    }
                    if (latenciesOut)
                        latenciesOut->push_back(ms);
                }

                unsigned const done = completed.fetch_add(1, std::memory_order_acq_rel) + 1;

                // Fire the next request if there is more work.
                if (launched.load(std::memory_order_relaxed) < count)
                    fireOne();

                if (done == count)
                    ioc.stop();

                return false;  // single-site, no retry
            },
            beast::Journal{beast::Journal::getNullSink()});
    };

    // Prime with `concurrency` in-flight requests.
    unsigned const primeCount = std::min(cfg.concurrency, count);
    for (unsigned i = 0; i < primeCount; ++i)
        fireOne();

    // Run io_context on clientThreads threads (spawn clientThreads-1 extras).
    std::vector<std::thread> threads;
    threads.reserve(cfg.clientThreads > 0 ? cfg.clientThreads - 1 : 0);
    for (unsigned i = 1; i < cfg.clientThreads; ++i)
        threads.emplace_back([&ioc]() { ioc.run(); });
    ioc.run();
    for (auto& t : threads)
        t.join();
}

// Run one closed-loop phase of `count` requests at `concurrency` on a fresh
// io_context, using the Rust HTTPRequestBuilder/asyncSubmit.
inline void
runRustPhase(
    RunConfig const& cfg,
    unsigned count,
    std::vector<double>* latenciesOut,
    unsigned& okOut,
    unsigned& errorsOut,
    std::string& firstErrorOut)
{
    boost::asio::io_context ioc;

    std::string const scheme = cfg.tls ? "https" : "http";
    std::string const url =
        scheme + "://" + cfg.host + ":" + std::to_string(cfg.port) + cfg.path;

    std::atomic<unsigned> launched{0};
    std::atomic<unsigned> completed{0};
    std::mutex mu;

    std::function<void()> fireOne;
    fireOne = [&]()
    {
        unsigned const slot = launched.fetch_add(1, std::memory_order_relaxed);
        if (slot >= count)
        {
            launched.fetch_sub(1, std::memory_order_relaxed);
            return;
        }

        auto const start = std::chrono::steady_clock::now();

        // A fresh named builder is required for each request: the setters
        // return HTTPRequestBuilder& (not a new builder), and asyncSubmit
        // moves the builder's internal state, so re-using a builder after
        // asyncSubmit is undefined behaviour.
        xrpl::HTTPRequestBuilder builder(url, ::rs::http_client::HTTPMethod::Get, cfg.timeout);
        builder.setMaxResponseSize(cfg.maxResponseBytes);
        if (cfg.requestConnectionClose)
            builder.addHeader("connection", "close");
        builder.asyncSubmit(
            ioc.get_executor(),
            [&, start](std::expected<::rs::http_client::Response, xrpl::HttpError> exp)
            {
                double const ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - start).count();

                {
                    std::lock_guard<std::mutex> lk(mu);
                    if (exp.has_value() && exp->status == 200)
                    {
                        ++okOut;
                    }
                    else
                    {
                        ++errorsOut;
                        if (firstErrorOut.empty())
                        {
                            if (!exp.has_value())
                                firstErrorOut = exp.error().message;
                            else
                                firstErrorOut = "status " + std::to_string(exp->status);
                        }
                    }
                    if (latenciesOut)
                        latenciesOut->push_back(ms);
                }

                unsigned const done = completed.fetch_add(1, std::memory_order_acq_rel) + 1;

                if (launched.load(std::memory_order_relaxed) < count)
                    fireOne();

                if (done == count)
                    ioc.stop();
            });
    };

    unsigned const primeCount = std::min(cfg.concurrency, count);
    for (unsigned i = 0; i < primeCount; ++i)
        fireOne();

    std::vector<std::thread> threads;
    threads.reserve(cfg.clientThreads > 0 ? cfg.clientThreads - 1 : 0);
    for (unsigned i = 1; i < cfg.clientThreads; ++i)
        threads.emplace_back([&ioc]() { ioc.run(); });
    ioc.run();
    for (auto& t : threads)
        t.join();
}

}  // namespace detail

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// NOTE: runLegacy assumes the caller has already invoked
//   xrpl::HTTPClient::initializeSSLContext(...)
// before calling this function when cfg.tls is true.  It does NOT call
// initializeSSLContext or cleanupSSLContext itself; those are process-wide
// resources whose lifetime the caller owns.
//
// NOTE: RSS and CPU measurements are process-wide (the in-process benchmark
// server contributes).  The server's contribution is common-mode across both
// clients, so the legacy-vs-Rust delta remains meaningful.

inline RunResult
runLegacy(RunConfig const& cfg)
{
    RunResult result;

    // Warmup phase — fresh io_context, no recording.
    if (cfg.warmupRequests > 0)
    {
        unsigned wOk = 0, wErr = 0;
        std::string wErr1;
        detail::runLegacyPhase(cfg, cfg.warmupRequests, nullptr, wOk, wErr, wErr1);
    }

    // Capture baseline resource usage immediately before the measured phase.
    struct rusage before{}, after{};
    ::getrusage(RUSAGE_SELF, &before);

    auto const wallStart = std::chrono::steady_clock::now();

    // Measured phase.
    std::vector<double> latencies;
    latencies.reserve(cfg.totalRequests);
    detail::runLegacyPhase(
        cfg, cfg.totalRequests, &latencies, result.ok, result.errors, result.firstError);

    auto const wallEnd = std::chrono::steady_clock::now();
    ::getrusage(RUSAGE_SELF, &after);

    result.wallSeconds = std::chrono::duration<double>(wallEnd - wallStart).count();

    result.cpuUserSeconds =
        detail::timevalToSeconds(after.ru_utime) - detail::timevalToSeconds(before.ru_utime);
    result.cpuSysSeconds =
        detail::timevalToSeconds(after.ru_stime) - detail::timevalToSeconds(before.ru_stime);

    // ru_maxrss: on Apple it is already in bytes; on Linux it is kilobytes.
#if defined(__APPLE__)
    result.peakRssBytes = after.ru_maxrss;
#else
    result.peakRssBytes = after.ru_maxrss * 1024L;
#endif

    detail::fillStats(result, latencies);
    return result;
}

// NOTE: runRust assumes the caller has already invoked
//   ::rs::http_client::init_tokio_runtime(N)
// and
//   ::rs::http_client::init_tls_context(cfg)
// (with the desired disable_connection_reuse setting) before calling this
// function.  runRust does NOT touch the Tokio runtime or the TLS context;
// those are process-wide resources whose lifetime the caller owns.
//
// NOTE: RSS and CPU measurements are process-wide — see comment above runLegacy.

inline RunResult
runRust(RunConfig const& cfg)
{
    RunResult result;

    // Warmup phase — fresh io_context, no recording.
    if (cfg.warmupRequests > 0)
    {
        unsigned wOk = 0, wErr = 0;
        std::string wErr1;
        detail::runRustPhase(cfg, cfg.warmupRequests, nullptr, wOk, wErr, wErr1);
    }

    // Capture baseline resource usage immediately before the measured phase.
    struct rusage before{}, after{};
    ::getrusage(RUSAGE_SELF, &before);

    auto const wallStart = std::chrono::steady_clock::now();

    // Measured phase.
    std::vector<double> latencies;
    latencies.reserve(cfg.totalRequests);
    detail::runRustPhase(
        cfg, cfg.totalRequests, &latencies, result.ok, result.errors, result.firstError);

    auto const wallEnd = std::chrono::steady_clock::now();
    ::getrusage(RUSAGE_SELF, &after);

    result.wallSeconds = std::chrono::duration<double>(wallEnd - wallStart).count();

    result.cpuUserSeconds =
        detail::timevalToSeconds(after.ru_utime) - detail::timevalToSeconds(before.ru_utime);
    result.cpuSysSeconds =
        detail::timevalToSeconds(after.ru_stime) - detail::timevalToSeconds(before.ru_stime);

#if defined(__APPLE__)
    result.peakRssBytes = after.ru_maxrss;
#else
    result.peakRssBytes = after.ru_maxrss * 1024L;
#endif

    detail::fillStats(result, latencies);
    return result;
}

}  // namespace bench
