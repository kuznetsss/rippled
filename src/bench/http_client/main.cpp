#include "BenchCerts.h"
#include "BenchRunner.h"
#include "BenchServer.h"

#include <xrpl/beast/utility/Journal.h>
#include <xrpl/net/HTTPClient.h>
#include <rs_http_client_cxxbridge/ffi.h>

#include <boost/program_options.hpp>

#include <chrono>
#include <cstddef>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace po = boost::program_options;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static std::vector<unsigned>
parseConcurrencyList(std::string const& s)
{
    std::vector<unsigned> result;
    std::istringstream ss(s);
    std::string token;
    while (std::getline(ss, token, ','))
    {
        token.erase(0, token.find_first_not_of(" \t"));
        token.erase(token.find_last_not_of(" \t") + 1);
        if (!token.empty())
            result.push_back(static_cast<unsigned>(std::stoul(token)));
    }
    if (result.empty())
        throw std::runtime_error("--concurrency must be a non-empty comma-separated list");
    return result;
}

static std::string
nowIso()
{
    auto const now = std::chrono::system_clock::now();
    std::time_t const tt = std::chrono::system_clock::to_time_t(now);
    std::ostringstream oss;
    oss << std::put_time(std::gmtime(&tt), "%Y-%m-%dT%H:%M:%SZ");
    return oss.str();
}

// Format a double to `sig` significant figures without trailing zeros.
static std::string
fmtSig(double v, int sig = 3)
{
    if (v == 0.0)
        return "0";
    std::ostringstream oss;
    oss << std::setprecision(sig) << v;
    return oss.str();
}

static std::string
fmtMs(double ms)
{
    return fmtSig(ms, 3);
}

static std::string
fmtRps(double rps)
{
    std::ostringstream oss;
    oss << std::fixed << std::setprecision(1) << rps;
    return oss.str();
}

static std::string
fmtMB(long bytes)
{
    std::ostringstream oss;
    oss << std::fixed << std::setprecision(1) << (static_cast<double>(bytes) / (1024.0 * 1024.0));
    return oss.str();
}

// Compute CPU ms per request.
static double
cpuMsPerReq(bench::RunResult const& r)
{
    unsigned const total = r.ok + r.errors;
    if (total == 0)
        return 0.0;
    return (r.cpuUserSeconds + r.cpuSysSeconds) * 1000.0 / static_cast<double>(total);
}

// Markdown table row helper (pipes are caller-supplied separators).
static std::string
tableRow(std::vector<std::string> const& cells)
{
    std::ostringstream oss;
    oss << "|";
    for (auto const& c : cells)
        oss << " " << c << " |";
    return oss.str();
}

static std::string
tableHeader(std::vector<std::string> const& cols)
{
    std::string header = tableRow(cols);
    std::string sep = "|";
    for (std::size_t i = 0; i < cols.size(); ++i)
        sep += "---|";
    return header + "\n" + sep;
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

struct RunRecord
{
    std::string transport;   // "http" or "https"
    unsigned concurrency;
    std::string label;       // "Legacy", "Rust(reuse-on)", "Rust(forced-close)"
    bench::RunResult result;
};

static void
writeReport(
    std::ostream& out,
    std::vector<RunRecord> const& records,
    unsigned requests,
    unsigned warmup,
    std::size_t responseSize,
    std::vector<std::string> const& transports,
    unsigned serverThreads,
    unsigned legacyThreads,
    unsigned tokioThreads,
    unsigned controlThreads,
    bool tlsVerify,
    std::string const& timestamp)
{
    out << "# HTTP Client Benchmark Report\n\n";
    out << "Generated: " << timestamp << "\n\n";

    // What this models
    out << "## What this models\n\n";
    out << "This benchmark models `ValidatorSite`'s ~5-minute-cadence fresh-connection HTTPS "
           "fetches (and one-shot CLI RPC via `RPCCall`). In production, the connection pool "
           "is effectively never warm: keep-alive idle connections time out in ~60–120 s but "
           "fetches are 5 minutes apart. Real concurrency is ~1 (a handful of staggered sites "
           "at most). Therefore:\n\n";
    out << "- **Rust(forced-close)** is the realistic primary comparison vs Legacy — both pay "
           "a fresh TCP connect + TLS handshake per request, matching production.\n";
    out << "- **Rust(reuse-on)** is an upper-bound ceiling that only applies if requests are "
           "frequent or bursty enough to reuse a warm connection, which does not happen at the "
           "5-minute cadence. It is kept for reference but is NOT the headline result.\n";
    out << "- Metrics emphasize **per-request latency, CPU/req, and RSS** over throughput, "
           "because the client is a low-rate background fetcher — req/s is not meaningful for "
           "this workload.\n";
    out << "- **HTTPS is the primary transport** (validator lists are always HTTPS); "
           "HTTP is a secondary baseline.\n";
    out << "\n";

    // Setup section
    out << "## Setup\n\n";
    out << "| Parameter | Value |\n";
    out << "|---|---|\n";
    out << "| Measured requests | " << requests << " |\n";
    out << "| Warmup requests | " << warmup << " |\n";
    out << "| Response body size | " << responseSize << " bytes |\n";
    out << "| Transports | ";
    for (std::size_t i = 0; i < transports.size(); ++i)
    {
        if (i > 0)
            out << ", ";
        out << transports[i];
    }
    out << " |\n";
    out << "| Server threads | " << serverThreads << " |\n";
    out << "| Legacy client threads (network runtime + control) | " << legacyThreads
        << " + " << controlThreads << " = " << (legacyThreads + controlThreads) << " total |\n";
    out << "| Rust client threads (Tokio runtime + control) | " << tokioThreads
        << " + " << controlThreads << " = " << (tokioThreads + controlThreads) << " total |\n";
    out << "| TLS verify | " << (tlsVerify ? "yes" : "no") << " |\n";
    out << "| Machine | " << "loopback (in-process server + client)" << " |\n";
    out << "\n";

    // Cells explanation
    out << "## Cells\n\n";
    out << "- **Legacy** — `xrpl::HTTPClient::get` with `Connection: close`; "
           "always pays a fresh TCP connection (and TLS handshake) per request. "
           "This is how the legacy client behaves in production.\n";
    out << "- **Rust(forced-close)** (Cell B) — `reqwest` with `disable_connection_reuse=true` "
           "*and* a `Connection: close` request header, so the server initiates each close — "
           "same fresh-connection semantics as Legacy. **This is the PRIMARY realistic comparison**: "
           "both clients pay a fresh TCP connect + TLS handshake, matching the 5-minute-cadence "
           "production workload. The close header also keeps TIME_WAIT on the server's fixed port "
           "instead of exhausting client ephemeral ports on loopback.\n";
    out << "- **Rust(reuse-on)** (Cell A) — `reqwest` with default connection pooling / keep-alive. "
           "After the first request the TCP connection (and TLS session) is reused. "
           "**This is a bursty-only upper-bound ceiling** that does NOT reflect production's "
           "5-minute fetch cadence (the pool expires before the next fetch). Keep for reference only.\n";
    out << "\n";

    // Per-transport tables
    for (auto const& transport : transports)
    {
        out << "## Results — " << transport;
        if (transport == "https")
            out << " (primary transport)";
        out << "\n\n";

        // Headline columns: latency first, then CPU/req, RSS, Errors, throughput last.
        std::vector<std::string> cols = {
            "Concurrency", "Client",
            "p50 (ms)", "p90 (ms)", "p99 (ms)", "max (ms)", "mean (ms)",
            "CPU ms/req", "Peak RSS (MB)", "Errors",
            "Throughput (req/s)", "Notes"
        };
        out << tableHeader(cols) << "\n";

        for (auto const& rec : records)
        {
            if (rec.transport != transport)
                continue;

            auto const& r = rec.result;
            std::string notes;
            if (rec.label == "Rust(reuse-on)")
                notes = "bursty ceiling only";
            else if (!r.firstError.empty())
                notes = r.firstError;

            // For fresh-connection cells, display throughput as concurrency/mean-latency
            // (Little's Law over the bounded sample) rather than the raw measured req/s, which
            // is not a sustainable-capacity figure at low concurrency.
            double displayedRps = r.throughputRps;
            bool const isFreshConn = (rec.label != "Rust(reuse-on)");
            if (isFreshConn && r.meanMs > 0.0)
                displayedRps = static_cast<double>(rec.concurrency) / (r.meanMs / 1000.0);

            out << tableRow({
                std::to_string(rec.concurrency),
                rec.label,
                fmtMs(r.p50Ms),
                fmtMs(r.p90Ms),
                fmtMs(r.p99Ms),
                fmtMs(r.maxMs),
                fmtMs(r.meanMs),
                fmtSig(cpuMsPerReq(r), 3),
                fmtMB(r.peakRssBytes),
                std::to_string(r.errors),
                fmtRps(displayedRps),
                notes
            }) << "\n";
        }
        out << "\n";
        out << "> Throughput for fresh-connection cells (Legacy, Rust(forced-close)) is derived "
               "as `concurrency / mean-latency` (Little's Law) over the bounded sample — it "
               "reflects per-request cost, not sustained capacity. Only Rust(reuse-on) gives a "
               "clean sustained-throughput figure.\n";
        out << "\n";

        // Latency and CPU ratios (replaces the old "Speedup vs Legacy" throughput ratios)
        out << "### Improvement vs Legacy (" << transport << ")\n\n";
        out << "> Ratio > 1 means Rust is faster/cheaper. "
               "Rust(forced-close) is the realistic primary; Rust(reuse-on) is the bursty ceiling.\n\n";

        // Collect unique concurrency levels for this transport.
        std::vector<unsigned> concLevels;
        for (auto const& rec : records)
        {
            if (rec.transport == transport)
            {
                bool found = false;
                for (auto c : concLevels)
                    if (c == rec.concurrency)
                    {
                        found = true;
                        break;
                    }
                if (!found)
                    concLevels.push_back(rec.concurrency);
            }
        }

        std::vector<std::string> ratioCols = {
            "Concurrency",
            "Legacy/Rust-B p50 latency", "Legacy/Rust-B CPU/req",
            "Legacy/Rust-A p50 latency", "Legacy/Rust-A CPU/req"
        };
        out << tableHeader(ratioCols) << "\n";

        for (unsigned conc : concLevels)
        {
            double legacyP50 = 0.0, legacyCpu = 0.0;
            double rustAP50 = 0.0, rustACpu = 0.0;
            double rustBP50 = 0.0, rustBCpu = 0.0;
            for (auto const& rec : records)
            {
                if (rec.transport != transport || rec.concurrency != conc)
                    continue;
                if (rec.label == "Legacy")
                {
                    legacyP50 = rec.result.p50Ms;
                    legacyCpu = cpuMsPerReq(rec.result);
                }
                else if (rec.label == "Rust(reuse-on)")
                {
                    rustAP50 = rec.result.p50Ms;
                    rustACpu = cpuMsPerReq(rec.result);
                }
                else if (rec.label == "Rust(forced-close)")
                {
                    rustBP50 = rec.result.p50Ms;
                    rustBCpu = cpuMsPerReq(rec.result);
                }
            }

            auto fmtRatio = [](double num, double den) -> std::string
            {
                if (den <= 0.0)
                    return "n/a";
                return fmtSig(num / den, 3) + "x";
            };

            out << tableRow({
                std::to_string(conc),
                fmtRatio(legacyP50, rustBP50),
                fmtRatio(legacyCpu, rustBCpu),
                fmtRatio(legacyP50, rustAP50),
                fmtRatio(legacyCpu, rustACpu)
            }) << "\n";
        }
        out << "\n";
    }

    // Caveats section
    out << "## Caveats\n\n";
    out << "- **In-process server**: RSS and CPU measurements are process-wide and include "
           "the benchmark server. The server contribution is common-mode across both clients, "
           "so the legacy-vs-Rust *delta* remains meaningful.\n";
    out << "- **No core pinning**: macOS does not expose `pthread_setaffinity_np`; "
           "both clients may experience scheduler interference.\n";
    out << "- **Legacy is GET with Connection: close**: the legacy client always negotiates "
           "a fresh connection per request regardless of transport.\n";
    out << "- **Warmup excluded**: the first " << warmup << " requests are unmeasured "
           "and excluded from all statistics.\n";
    out << "- **Fresh server per run**: every run spins up its own BenchServer on a new "
           "ephemeral port, so each gets an isolated TCP 4-tuple space + fresh server threads "
           "and a prior run's TIME_WAIT sockets cannot starve it.\n";
    out << "- **Legacy-only driver watchdog**: the legacy client has no working per-request "
           "timeout in its success path (its deadline timer is only armed on an exception branch), "
           "so a backstop timer is kept on the legacy path only. reqwest has a working per-request "
           "timeout and the completion always fires, so no watchdog is needed on the Rust path. "
           "Watchdog timeouts appear in the Errors column.\n";
    out << "- **Symmetric thread split**: each client runs N runtime threads + "
        << controlThreads << " control thread(s). Legacy = " << legacyThreads
        << " network io_context + " << controlThreads << " control = "
        << (legacyThreads + controlThreads) << "; Rust = " << tokioThreads
        << " Tokio + " << controlThreads << " control = " << (tokioThreads + controlThreads)
        << ". The runtime does network I/O; the control io_context fires requests and "
           "records stats, with completions hopped runtime->control in BOTH paths. "
           "Adjust `--legacy-threads`/`--tokio-threads`/`--control-threads` to taste.\n";
    out << "- **Throughput note**: for fresh-connection cells, displayed throughput is "
           "`concurrency / mean-latency` (Little's Law), not sustained capacity. At the "
           "production cadence (~1 req per 5 min per site), throughput is irrelevant; "
           "per-request latency and CPU cost are what matter.\n";
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

int
main(int argc, char* argv[])
{
    // --- CLI ---
    po::options_description desc("HTTP client benchmark options");
    // clang-format off
    desc.add_options()
        ("help,h",                                                      "show help")
        ("requests",         po::value<unsigned>()->default_value(2000),
                             // Kept modest: fresh-connection runs must not approach ephemeral-port
                             // exhaustion (~64k ports, TIME_WAIT ~60s). 2000 requests at c=8 is
                             // well within the safe zone on loopback.
                             "measured requests per run")
        ("warmup",           po::value<unsigned>()->default_value(200),
                             "unmeasured warmup requests per run")
        ("concurrency",      po::value<std::string>()->default_value("1,8"),
                             // 1 models a single periodic fetch (ValidatorSite's ~5-min cadence);
                             // 8 models the worst case of several sites refreshing near-simultaneously.
                             // High concurrency (32/64) models a workload this client never has.
                             "comma-separated concurrency levels to sweep")
        ("response-size",    po::value<std::size_t>()->default_value(1024),
                             "server response body size in bytes")
        ("transport",        po::value<std::string>()->default_value("both"),
                             // https is the primary transport (validator lists are always HTTPS);
                             // http is kept as a secondary baseline.
                             "http | https | both")
        ("server-threads",   po::value<unsigned>()->default_value(8),
                             "server io_context thread pool size")
        ("legacy-threads",   po::value<unsigned>()->default_value(2),
                             "legacy client network (runtime) io_context threads")
        ("tokio-threads",    po::value<unsigned>()->default_value(2),
                             "Tokio worker (runtime) threads for the Rust client")
        ("control-threads",  po::value<unsigned>()->default_value(1),
                             "control/dispatch threads driving the closed loop (both clients)")
        ("tls-verify",       po::value<bool>()->default_value(true),
                             "verify server cert against embedded CA")
        ("timeout-seconds",  po::value<unsigned>()->default_value(30),
                             "per-request timeout in seconds")
        ("max-response-bytes", po::value<std::size_t>()->default_value(8 * 1024 * 1024),
                             "response size cap passed to both clients")
        ("out",              po::value<std::string>()->default_value("http_client_bench_report.md"),
                             "output Markdown report path");
    // clang-format on

    po::variables_map vm;
    try
    {
        po::store(po::parse_command_line(argc, argv, desc), vm);
        po::notify(vm);
    }
    catch (std::exception const& ex)
    {
        std::cerr << "Error: " << ex.what() << "\n" << desc << "\n";
        return 1;
    }

    if (vm.count("help"))
    {
        std::cout << desc << "\n";
        return 0;
    }

    unsigned const requests          = vm["requests"].as<unsigned>();
    unsigned const warmup            = vm["warmup"].as<unsigned>();
    std::string const concurrencyStr = vm["concurrency"].as<std::string>();
    std::size_t const responseSize   = vm["response-size"].as<std::size_t>();
    std::string const transportStr   = vm["transport"].as<std::string>();
    unsigned const serverThreads     = vm["server-threads"].as<unsigned>();
    unsigned const legacyThreads     = vm["legacy-threads"].as<unsigned>();
    unsigned const tokioThreads      = vm["tokio-threads"].as<unsigned>();
    unsigned const controlThreads    = vm["control-threads"].as<unsigned>();
    bool const tlsVerify             = vm["tls-verify"].as<bool>();
    unsigned const timeoutSec        = vm["timeout-seconds"].as<unsigned>();
    std::size_t const maxRespBytes   = vm["max-response-bytes"].as<std::size_t>();
    std::string const outPath        = vm["out"].as<std::string>();

    // Validate / parse transport
    bool const doHttp  = (transportStr == "http"  || transportStr == "both");
    bool const doHttps = (transportStr == "https" || transportStr == "both");
    if (!doHttp && !doHttps)
    {
        std::cerr << "Error: --transport must be http, https, or both\n";
        return 1;
    }
    bool const enableTls = doHttps;

    // Parse concurrency list
    std::vector<unsigned> concLevels;
    try
    {
        concLevels = parseConcurrencyList(concurrencyStr);
    }
    catch (std::exception const& ex)
    {
        std::cerr << "Error: " << ex.what() << "\n";
        return 1;
    }

    // https is listed first: it is the primary transport (ValidatorSite always
    // uses HTTPS); http follows as a secondary baseline.
    std::vector<std::string> activeTransports;
    if (doHttps)
        activeTransports.push_back("https");
    if (doHttp)
        activeTransports.push_back("http");

    std::string const timestamp = nowIso();

    // A fresh BenchServer is created per run inside the loop below (see "test
    // case" scoping there), so each run gets its own ephemeral port + server
    // threads. `enableTls` only gates whether https runs happen.

    // --- Materialize embedded CA cert to a temp file (if TLS verify requested) ---
    std::filesystem::path caTempPath;
    if (tlsVerify && enableTls)
    {
        caTempPath = std::filesystem::temp_directory_path() / "rippled_bench_ca.pem";
        {
            std::ofstream f(caTempPath, std::ios::trunc | std::ios::binary);
            if (!f)
                throw std::runtime_error("cannot write temp CA file: " + caTempPath.string());
            f.write(
                bench::certs::kCaCertPem.data(),
                static_cast<std::streamsize>(bench::certs::kCaCertPem.size()));
        }
    }

    // --- Init Tokio runtime ---
    {
        auto const status = ::rs::http_client::init_tokio_runtime(tokioThreads);
        if (status.code != ::rs::http_client::ErrorCode::Ok &&
            status.code != ::rs::http_client::ErrorCode::AlreadyInitialized)
        {
            std::cerr << "Error: init_tokio_runtime failed: "
                      << static_cast<std::string>(status.message) << "\n";
            return 1;
        }
    }

    // --- Init legacy SSL context ---
    // The legacy HTTPClientImp constructs its socket against the global SSL
    // context for EVERY request, even plain HTTP, so it must be initialized
    // before any legacy run regardless of transport (it is only actually used
    // for the TLS handshake on https runs).
    {
        std::string const sslVerifyDir;
        std::string const sslVerifyFile = (tlsVerify && !caTempPath.empty())
            ? caTempPath.string()
            : std::string{};
        xrpl::HTTPClient::initializeSSLContext(
            sslVerifyDir, sslVerifyFile, tlsVerify, beast::Journal{beast::Journal::getNullSink()});
    }

    // ---------------------------------------------------------------------------
    // Run loop
    // ---------------------------------------------------------------------------

    std::vector<RunRecord> allRecords;

    auto const runLabel = [&](
        std::string const& transport,
        unsigned conc,
        std::string const& label,
        bench::RunConfig const& cfg)
    {
        std::cerr << "running " << transport << " c=" << conc << " " << label << "...\n";
        bench::RunResult result = (cfg.client == bench::ClientKind::Legacy)
            ? bench::runLegacy(cfg)
            : bench::runRust(cfg);
        allRecords.push_back({transport, conc, label, std::move(result)});
    };

    auto const initRustTls = [&](bool disableReuse)
    {
        ::rs::http_client::reset_tls_context();
        ::rs::http_client::TlsConfig cfg{};
        cfg.verify = tlsVerify;
        cfg.verify_file = (tlsVerify && !caTempPath.empty())
            ? rust::String(caTempPath.string())
            : rust::String{};
        cfg.verify_dir = rust::String{};
        cfg.disable_connection_reuse = disableReuse;
        auto const status = ::rs::http_client::init_tls_context(cfg);
        if (status.code != ::rs::http_client::ErrorCode::Ok)
        {
            std::cerr << "Warning: init_tls_context failed: "
                      << static_cast<std::string>(status.message) << "\n";
        }
    };

    auto const timeout = std::chrono::milliseconds(
        static_cast<std::chrono::milliseconds::rep>(timeoutSec) * 1000);

    for (auto const& transport : activeTransports)
    {
        bool const isTls = (transport == "https");

        for (unsigned conc : concLevels)
        {
            // Each run ("test case") spins up its own BenchServer on a fresh
            // ephemeral port, so it gets an isolated TCP 4-tuple space and fresh
            // server threads — nothing carries over from the previous run.

            // --- Legacy ---
            {
                bench::BenchServer server(serverThreads, responseSize, isTls);
                bench::RunConfig cfg;
                cfg.client           = bench::ClientKind::Legacy;
                cfg.tls              = isTls;
                cfg.host             = "127.0.0.1";
                cfg.port             = server.port();
                cfg.path             = "/bench";
                cfg.maxResponseBytes = maxRespBytes;
                cfg.totalRequests    = requests;
                cfg.warmupRequests   = warmup;
                cfg.concurrency      = conc;
                cfg.ioThreads        = legacyThreads;
                cfg.controlThreads   = controlThreads;
                cfg.timeout          = timeout;
                runLabel(transport, conc, "Legacy", cfg);
            }

            // --- Rust Cell A: reuse on ---
            {
                initRustTls(/*disableReuse=*/false);
                bench::BenchServer server(serverThreads, responseSize, isTls);
                bench::RunConfig cfg;
                cfg.client           = bench::ClientKind::Rust;
                cfg.tls              = isTls;
                cfg.host             = "127.0.0.1";
                cfg.port             = server.port();
                cfg.path             = "/bench";
                cfg.maxResponseBytes = maxRespBytes;
                cfg.totalRequests    = requests;
                cfg.warmupRequests   = warmup;
                cfg.concurrency      = conc;
                cfg.ioThreads        = tokioThreads;
                cfg.controlThreads   = controlThreads;
                cfg.timeout          = timeout;
                runLabel(transport, conc, "Rust(reuse-on)", cfg);
            }

            // --- Rust Cell B: forced close ---
            {
                initRustTls(/*disableReuse=*/true);
                bench::BenchServer server(serverThreads, responseSize, isTls);
                bench::RunConfig cfg;
                cfg.client           = bench::ClientKind::Rust;
                cfg.tls              = isTls;
                cfg.host             = "127.0.0.1";
                cfg.port             = server.port();
                cfg.path             = "/bench";
                cfg.maxResponseBytes = maxRespBytes;
                cfg.totalRequests    = requests;
                cfg.warmupRequests   = warmup;
                cfg.concurrency      = conc;
                cfg.ioThreads        = tokioThreads;
                cfg.controlThreads   = controlThreads;
                cfg.timeout          = timeout;
                cfg.requestConnectionClose = true;
                runLabel(transport, conc, "Rust(forced-close)", cfg);
            }
        }
    }

    // --- Teardown ---
    ::rs::http_client::shutdown_tokio_runtime(2000);
    xrpl::HTTPClient::cleanupSSLContext();
    if (!caTempPath.empty())
        std::filesystem::remove(caTempPath);

    // --- Write report ---
    {
        std::ofstream outFile(outPath, std::ios::trunc);
        if (!outFile)
        {
            std::cerr << "Error: cannot open report file: " << outPath << "\n";
            return 1;
        }
        writeReport(
            outFile,
            allRecords,
            requests,
            warmup,
            responseSize,
            activeTransports,
            serverThreads,
            legacyThreads,
            tokioThreads,
            controlThreads,
            tlsVerify,
            timestamp);
        std::cout << "Report written to: " << outPath << "\n";
    }

    return 0;
}
