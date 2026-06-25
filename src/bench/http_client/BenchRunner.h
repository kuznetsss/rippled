#pragma once

#include <xrpl/beast/utility/Journal.h>
#include <xrpl/net/HTTPClient.h>
#include <xrpl/net/HTTPClientRust.h>
#include <rs_http_client_cxxbridge/ffi.h>

#include <boost/asio/executor_work_guard.hpp>
#include <boost/asio/io_context.hpp>
#include <boost/asio/post.hpp>
#include <boost/asio/steady_timer.hpp>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <expected>
#include <functional>
#include <mutex>
#include <numeric>
#include <ostream>
#include <string>
#include <sys/resource.h>
#include <thread>
#include <utility>
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
    // Thread split, kept symmetric between the two clients:
    //   ioThreads      — the async "runtime" doing network I/O. For Legacy this
    //                    is the network io_context's thread pool; for Rust the
    //                    Tokio worker pool (set globally via init_tokio_runtime,
    //                    so runRust does not spawn these itself — the field is
    //                    informational there).
    //   controlThreads — the closed-loop driver / completion-dispatch pool (an
    //                    Asio io_context for BOTH clients): fires the next
    //                    request and records stats. Completions are hopped from
    //                    an ioThread onto a controlThread in both paths.
    unsigned ioThreads;
    unsigned controlThreads;
    std::chrono::milliseconds timeout;
    // When true (the Rust forced-close cell), send `Connection: close` so the
    // SERVER initiates the TCP close — mirroring the legacy client and keeping
    // the active-close (and thus TIME_WAIT) on the server's fixed port rather
    // than exhausting the client's ephemeral ports on loopback. Ignored by the
    // legacy path, which always sends `Connection: close`.
    bool requestConnectionClose = false;
    // Typical request payload, mirroring the heaviest real call (the JSON-RPC
    // POST built in RPCCall.cpp): the app-level header set (e.g. User-Agent /
    // Content-Type / Accept) plus a body. `Host` and `Content-Length` are NOT
    // listed here — they are added per-transport (written by the legacy raw
    // builder, computed by reqwest). When `requestBody` is non-empty the request
    // is issued as POST; otherwise it stays a bodyless GET.
    std::vector<std::pair<std::string, std::string>> requestHeaders;
    std::string requestBody;
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

// Run one closed-loop phase of `count` requests at `concurrency`, using the
// legacy HTTPClient::get. Latencies are appended to `latenciesOut` when
// non-null (nullptr => warmup).
//
// Two io_contexts keep the legacy thread split symmetric with the Rust path:
//   netIoc     — the async "runtime": resolve / connect / TLS / read / write,
//                run on `cfg.ioThreads` threads.
//   controlIoc — the closed-loop driver, run on `cfg.controlThreads` threads:
//                fires the next request and records stats.
// HTTPClient::get invokes its completion on netIoc, so the completion hops the
// result onto controlIoc via post() — exactly as the Rust path hops Tokio
// completions onto its Asio dispatch io_context. Without that hop the control
// work would run on the network threads and the split would be meaningless.
inline void
runLegacyPhase(
    RunConfig const& cfg,
    unsigned count,
    std::vector<double>* latenciesOut,
    unsigned& okOut,
    unsigned& errorsOut,
    std::string& firstErrorOut)
{
    boost::asio::io_context netIoc;
    boost::asio::io_context controlIoc;
    // Both contexts must outlive the in-flight requests: netIoc between bursts,
    // controlIoc between the primed fires and the first posted completions.
    auto netGuard = boost::asio::make_work_guard(netIoc);
    auto controlGuard = boost::asio::make_work_guard(controlIoc);

    std::atomic<unsigned> launched{0};
    std::atomic<unsigned> completed{0};
    std::mutex mu;

    auto const timeoutSec =
        std::chrono::ceil<std::chrono::seconds>(cfg.timeout) < std::chrono::seconds(1)
        ? std::chrono::seconds(1)
        : std::chrono::ceil<std::chrono::seconds>(cfg.timeout);

    // Driver watchdog: a per-request backstop set just beyond the client's own
    // timeout. The legacy client has no working per-request timeout in its
    // success path (its deadline timer is only armed on an exception branch), so
    // without this a single stalled connection wedges the whole phase. When it
    // fires, the request is recorded as a timeout error and the loop proceeds.
    auto const watchdog = cfg.timeout + std::chrono::seconds(5);

    std::function<void()> fireOne;

    auto stopAll = [&]()
    {
        netGuard.reset();
        controlGuard.reset();
        netIoc.stop();
        controlIoc.stop();
    };

    // Records one settled request and advances the closed loop. Runs on a
    // control thread.
    auto recordResult = [&](bool ok, std::string msg, double ms)
    {
        {
            std::lock_guard<std::mutex> lk(mu);
            if (ok)
            {
                ++okOut;
            }
            else
            {
                ++errorsOut;
                if (firstErrorOut.empty())
                    firstErrorOut = std::move(msg);
            }
            if (latenciesOut)
                latenciesOut->push_back(ms);
        }

        unsigned const done = completed.fetch_add(1, std::memory_order_acq_rel) + 1;
        if (launched.load(std::memory_order_relaxed) < count)
            fireOne();
        if (done == count)
            stopAll();
    };

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
        // settle-once: whichever of {real completion, watchdog} fires first wins.
        auto settled = std::make_shared<std::atomic<bool>>(false);
        auto timer = std::make_shared<boost::asio::steady_timer>(controlIoc);

        timer->expires_after(watchdog);
        timer->async_wait(
            [&recordResult, settled, start](boost::system::error_code const& ec)
            {
                if (ec)  // cancelled by a real completion
                    return;
                if (settled->exchange(true))
                    return;
                double const ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - start).count();
                recordResult(false, "request timed out (driver watchdog)", ms);
            });

        xrpl::HTTPClient::request(
            cfg.tls,
            netIoc,
            cfg.host,
            cfg.port,
            [&cfg](boost::asio::streambuf& sb, std::string const& strHost)
            {
                // Mirror the JSON-RPC client's request (RPCCall.cpp): a POST
                // carrying the full typical header set plus a body. The legacy
                // client has no keep-alive, so it always sends `Connection:
                // close` and pays a fresh connection per request. `Host` and
                // `Content-Length` are written here; the rest come from cfg.
                std::ostream os(&sb);
                os << "POST " << (cfg.path.empty() ? "/" : cfg.path)
                   << " HTTP/1.0\r\n"
                   << "Host: " << strHost << ":" << cfg.port << "\r\n";
                for (auto const& [k, v] : cfg.requestHeaders)
                    os << k << ": " << v << "\r\n";
                os << "Content-Length: " << cfg.requestBody.size() << "\r\n"
                   << "Connection: close\r\n\r\n"
                   << cfg.requestBody;
            },
            cfg.maxResponseBytes,
            timeoutSec,
            [&recordResult, &controlIoc, settled, timer, start](
                boost::system::error_code const& ec,
                int status,
                std::string const& /*data*/) -> bool
            {
                // Runs on a network thread. Measure here, then hop the settle
                // onto the control io_context.
                double const ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - start).count();
                bool const ok = !ec && status == 200;
                std::string msg;
                if (!ok)
                    msg = ec ? ec.message() : ("status " + std::to_string(status));

                boost::asio::post(
                    controlIoc,
                    [&recordResult, settled, timer, ok, msg = std::move(msg), ms]() mutable
                    {
                        if (settled->exchange(true))
                            return;  // watchdog already settled this request
                        timer->cancel();
                        recordResult(ok, std::move(msg), ms);
                    });

                return false;  // single-site, no retry
            },
            beast::Journal{beast::Journal::getNullSink()});
    };

    // Prime with `concurrency` in-flight requests, fired on a control thread.
    unsigned const primeCount = std::min(cfg.concurrency, count);
    for (unsigned i = 0; i < primeCount; ++i)
        boost::asio::post(controlIoc, [&fireOne]() { fireOne(); });

    // Network "runtime" threads.
    std::vector<std::thread> netThreads;
    netThreads.reserve(cfg.ioThreads);
    for (unsigned i = 0; i < cfg.ioThreads; ++i)
        netThreads.emplace_back([&netIoc]() { netIoc.run(); });

    // Control threads: this thread + (controlThreads - 1) extras.
    std::vector<std::thread> controlThreads;
    controlThreads.reserve(cfg.controlThreads > 0 ? cfg.controlThreads - 1 : 0);
    for (unsigned i = 1; i < cfg.controlThreads; ++i)
        controlThreads.emplace_back([&controlIoc]() { controlIoc.run(); });
    controlIoc.run();

    for (auto& t : controlThreads)
        t.join();
    for (auto& t : netThreads)
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
    auto controlGuard = boost::asio::make_work_guard(ioc);

    std::string const scheme = cfg.tls ? "https" : "http";
    std::string const url =
        scheme + "://" + cfg.host + ":" + std::to_string(cfg.port) + cfg.path;

    std::atomic<unsigned> launched{0};
    std::atomic<unsigned> completed{0};
    std::mutex mu;

    // Backstop for a stalled request (see runLegacyPhase). reqwest has its own
    // timeout, so this normally only catches true hangs; the margin lets the
    // client's own timeout fire first and report a proper Timeout.
    auto const watchdog = cfg.timeout + std::chrono::seconds(5);

    std::function<void()> fireOne;

    auto stopAll = [&]()
    {
        controlGuard.reset();
        ioc.stop();
    };

    // Records one settled request and advances the closed loop. Runs on the
    // control io_context.
    auto recordResult = [&](bool ok, std::string msg, double ms)
    {
        {
            std::lock_guard<std::mutex> lk(mu);
            if (ok)
            {
                ++okOut;
            }
            else
            {
                ++errorsOut;
                if (firstErrorOut.empty())
                    firstErrorOut = std::move(msg);
            }
            if (latenciesOut)
                latenciesOut->push_back(ms);
        }

        unsigned const done = completed.fetch_add(1, std::memory_order_acq_rel) + 1;
        if (launched.load(std::memory_order_relaxed) < count)
            fireOne();
        if (done == count)
            stopAll();
    };

    fireOne = [&]()
    {
        unsigned const slot = launched.fetch_add(1, std::memory_order_relaxed);
        if (slot >= count)
        {
            launched.fetch_sub(1, std::memory_order_relaxed);
            return;
        }

        auto const start = std::chrono::steady_clock::now();
        auto settled = std::make_shared<std::atomic<bool>>(false);
        auto timer = std::make_shared<boost::asio::steady_timer>(ioc);

        timer->expires_after(watchdog);
        timer->async_wait(
            [&recordResult, settled, start](boost::system::error_code const& ec)
            {
                if (ec)
                    return;
                if (settled->exchange(true))
                    return;
                double const ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - start).count();
                recordResult(false, "request timed out (driver watchdog)", ms);
            });

        // A fresh named builder is required for each request: the setters
        // return HTTPRequestBuilder& (not a new builder), and asyncSubmit
        // moves the builder's internal state, so re-using a builder after
        // asyncSubmit is undefined behaviour.
        // POST when there is a body, otherwise a bodyless GET — matching the
        // legacy path. reqwest adds `Host` (from the URL) and `Content-Length`
        // (from the body) itself, so we only add the app-level headers.
        auto const method = cfg.requestBody.empty()
            ? ::rs::http_client::HTTPMethod::Get
            : ::rs::http_client::HTTPMethod::Post;
        xrpl::HTTPRequestBuilder builder(url, method, cfg.timeout);
        builder.setMaxResponseSize(cfg.maxResponseBytes);
        for (auto const& [k, v] : cfg.requestHeaders)
            builder.addHeader(k, v);
        if (cfg.requestConnectionClose)
            builder.addHeader("connection", "close");
        if (!cfg.requestBody.empty())
            builder.setBody(
                std::vector<uint8_t>(cfg.requestBody.begin(), cfg.requestBody.end()));
        builder.asyncSubmit(
            ioc.get_executor(),
            [&recordResult, settled, timer, start](
                std::expected<::rs::http_client::Response, xrpl::HttpError> exp)
            {
                // Posted onto the control io_context by HTTPCompletionImpl.
                if (settled->exchange(true))
                    return;  // watchdog already settled this request
                timer->cancel();

                double const ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - start).count();
                bool const ok = exp.has_value() && exp->status == 200;
                std::string msg;
                if (!ok)
                    msg = exp.has_value() ? ("status " + std::to_string(exp->status))
                                          : std::string(exp.error().message);
                recordResult(ok, std::move(msg), ms);
            });
    };

    unsigned const primeCount = std::min(cfg.concurrency, count);
    for (unsigned i = 0; i < primeCount; ++i)
        fireOne();

    // The Rust runtime (Tokio, cfg.ioThreads workers) is global; here we only
    // run the control/dispatch io_context that fires requests and receives the
    // completions posted back by HTTPCompletionImpl.
    std::vector<std::thread> threads;
    threads.reserve(cfg.controlThreads > 0 ? cfg.controlThreads - 1 : 0);
    for (unsigned i = 1; i < cfg.controlThreads; ++i)
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
