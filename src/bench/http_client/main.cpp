#include "BenchCerts.h"
#include "BenchRunner.h"
#include "BenchServer.h"

#include <xrpl/beast/utility/Journal.h>
#include <xrpl/net/HTTPClient.h>
#include <rs_http_client_cxxbridge/ffi.h>

#include <boost/program_options.hpp>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <numeric>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
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
fmtPct(double pct)
{
    std::ostringstream oss;
    oss << std::fixed << std::setprecision(2) << pct;
    return oss.str();
}

// Render a converged throughput as "20100.0 ± 0.83%" — the mean and its
// coefficient of variation (relative standard deviation) in one figure.
static std::string
fmtRpsCv(double rps, double cvPct)
{
    return fmtRps(rps) + " ± " + fmtPct(cvPct) + "%";
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

// ---------------------------------------------------------------------------
// Simple sample statistics over per-iteration throughput.
// ---------------------------------------------------------------------------

static double
sampleMean(std::vector<double> const& v)
{
    if (v.empty())
        return 0.0;
    return std::accumulate(v.begin(), v.end(), 0.0) / static_cast<double>(v.size());
}

// Sample standard deviation (N-1 / Bessel's correction). Needs >= 2 samples.
static double
sampleStdDev(std::vector<double> const& v)
{
    if (v.size() < 2)
        return 0.0;
    double const m = sampleMean(v);
    double acc = 0.0;
    for (double const x : v)
        acc += (x - m) * (x - m);
    return std::sqrt(acc / static_cast<double>(v.size() - 1));
}

// Coefficient of variation (relative standard deviation) in percent. This is
// the convergence metric: "1% standard deviation" => CV% <= 1.0.
static double
cvPct(std::vector<double> const& v)
{
    double const m = sampleMean(v);
    if (m == 0.0)
        return 0.0;
    return sampleStdDev(v) / m * 100.0;
}

// Relative standard error of the mean, in percent (reported for context).
static double
relStdErrPct(std::vector<double> const& v)
{
    if (v.size() < 2)
        return 0.0;
    double const m = sampleMean(v);
    if (m == 0.0)
        return 0.0;
    return (sampleStdDev(v) / std::sqrt(static_cast<double>(v.size()))) / m * 100.0;
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
// One cell of the comparison (a client configuration run once per iteration).
// ---------------------------------------------------------------------------

struct CellSpec
{
    std::string label;                  // shown in the report
    bench::ClientKind client;
    bool disableReuse = false;          // Rust only: connection pooling off
    bool requestConnectionClose = false;  // Rust only: send `Connection: close`
};

// ---------------------------------------------------------------------------
// Aggregated record for one (transport, concurrency, cell), pooled over all
// iterations of the convergence loop.
// ---------------------------------------------------------------------------

struct RunRecord
{
    std::string transport;   // "http" or "https"
    unsigned concurrency = 0;
    std::string label;       // "Legacy", "Rust(reuse-on)", "Rust(forced-close)"
    bench::RunResult result; // aggregated across iterations (mean throughput, etc.)
    unsigned iterations = 0;
    double throughputCvPct = 0.0;       // coefficient of variation of throughput
    double throughputRelStdErrPct = 0.0;
    bool converged = false;  // CV% reached the target within the iteration cap
};

// Aggregate the per-iteration RunResults for a single cell into one RunRecord.
//   - throughput  -> mean of per-iteration throughput (the converged figure)
//   - latencies   -> mean of per-iteration percentiles
//   - ok/errors   -> summed (so CPU ms/req = total CPU / total requests)
//   - CPU seconds -> summed
//   - peak RSS    -> max across iterations
static RunRecord
aggregate(
    std::string const& transport,
    unsigned concurrency,
    std::string const& label,
    std::vector<bench::RunResult> const& runs,
    double targetCvPct,
    unsigned minIterations)
{
    RunRecord rec;
    rec.transport = transport;
    rec.concurrency = concurrency;
    rec.label = label;
    rec.iterations = static_cast<unsigned>(runs.size());

    std::vector<double> tput;
    tput.reserve(runs.size());

    bench::RunResult agg;
    double sumP50 = 0, sumP90 = 0, sumP99 = 0, sumMax = 0, sumMean = 0;
    for (auto const& r : runs)
    {
        tput.push_back(r.throughputRps);
        agg.ok += r.ok;
        agg.errors += r.errors;
        agg.wallSeconds += r.wallSeconds;
        agg.cpuUserSeconds += r.cpuUserSeconds;
        agg.cpuSysSeconds += r.cpuSysSeconds;
        agg.peakRssBytes = std::max(agg.peakRssBytes, r.peakRssBytes);
        sumP50 += r.p50Ms;
        sumP90 += r.p90Ms;
        sumP99 += r.p99Ms;
        sumMax += r.maxMs;
        sumMean += r.meanMs;
        if (agg.firstError.empty() && !r.firstError.empty())
            agg.firstError = r.firstError;
    }

    double const n = runs.empty() ? 1.0 : static_cast<double>(runs.size());
    agg.throughputRps = sampleMean(tput);
    agg.p50Ms = sumP50 / n;
    agg.p90Ms = sumP90 / n;
    agg.p99Ms = sumP99 / n;
    agg.maxMs = sumMax / n;
    agg.meanMs = sumMean / n;

    rec.result = std::move(agg);
    rec.throughputCvPct = cvPct(tput);
    rec.throughputRelStdErrPct = relStdErrPct(tput);
    rec.converged = (rec.iterations >= minIterations) && (rec.throughputCvPct <= targetCvPct);
    return rec;
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

static void
writeReport(
    std::ostream& out,
    std::vector<RunRecord> const& records,
    unsigned requests,
    unsigned warmup,
    unsigned waitSeconds,
    double targetCvPct,
    unsigned minIterations,
    unsigned maxIterations,
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

    // Setup section
    out << "## Setup\n\n";
    out << "| Parameter | Value |\n";
    out << "|---|---|\n";
    out << "| Requests per iteration (per client) | " << requests << " |\n";
    out << "| Warmup requests (per iteration, per client) | " << warmup << " |\n";
    out << "| Inter-batch wait | " << waitSeconds << " s (lets TIME_WAIT sockets drain) |\n";
    out << "| Convergence target | throughput CV <= " << fmtPct(targetCvPct) << " % |\n";
    out << "| Iterations per level | min " << minIterations << ", max " << maxIterations << " |\n";
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

    // Methodology
    out << "## Methodology\n\n";
    out << "Each concurrency level is measured in repeated **batches**. One batch runs every "
           "client cell once, back to back, for `"
        << requests << "` requests each (C++ legacy first, then the Rust cells). After each "
           "batch the driver sleeps `"
        << waitSeconds << "`s so the kernel can reclaim the sockets left in TIME_WAIT before "
           "the next batch opens new ones. On loopback every fresh-connection request consumes "
           "2 sockets (client + server endpoint), and a batch runs 3 cells of `"
        << requests << "` requests, so a batch uses up to 2 x 3 x " << requests << " = "
        << (6 * requests) << " sockets — at the 10k default that is ~60k, right at the ceiling. "
           "Without the wait a long sweep exhausts the socket / ephemeral-port range (~60k) and "
           "ends up measuring socket-reclaim speed rather than client throughput.\n\n";
    out << "Per-batch throughput is collected as a sample. Batches repeat until the throughput "
           "coefficient of variation (std-dev / mean) for **every** cell is <= "
        << fmtPct(targetCvPct) << "% (with at least " << minIterations
        << " samples), or until " << maxIterations
        << " batches have run. Reported throughput is the mean over those batches expressed as "
           "`mean ± CV` (one relative standard deviation), so a converged row reads e.g. "
           "`20100.0 ± 0.83%`; latency percentiles are the per-batch mean; errors and CPU are "
           "summed; peak RSS is the max.\n\n";

    // Cells explanation
    out << "## Cells\n\n";
    out << "- **Legacy** — `xrpl::HTTPClient::get` with `Connection: close`; "
           "always pays a fresh TCP connection (and TLS handshake) per request.\n";
    out << "- **Rust(reuse-on)** (Cell A) — `reqwest` with default connection pooling / keep-alive. "
           "After the first request the TCP connection (and TLS session) is reused.\n";
    out << "- **Rust(forced-close)** (Cell B) — `reqwest` with `disable_connection_reuse=true` "
           "*and* a `Connection: close` request header, so the server initiates each close — "
           "same fresh-connection semantics as Legacy, isolating Rust's per-request overhead from "
           "pooling gains. (The close header also keeps TIME_WAIT on the server's fixed port "
           "instead of exhausting client ephemeral ports on loopback.)\n";
    out << "\n";

    // Per-transport tables
    for (auto const& transport : transports)
    {
        out << "## Results — " << transport << "\n\n";

        std::vector<std::string> cols = {
            "Concurrency", "Client", "Iters", "Throughput (req/s ± CV)",
            "p50 (ms)", "p90 (ms)", "p99 (ms)", "max (ms)",
            "CPU ms/req", "Peak RSS (MB)", "Errors", "Notes"
        };
        out << tableHeader(cols) << "\n";

        for (auto const& rec : records)
        {
            if (rec.transport != transport)
                continue;

            auto const& r = rec.result;
            std::string notes;
            if (!rec.converged)
                notes = "did not converge";
            if (!r.firstError.empty())
                notes += (notes.empty() ? "" : "; ") + r.firstError;

            out << tableRow({
                std::to_string(rec.concurrency),
                rec.label,
                std::to_string(rec.iterations),
                fmtRpsCv(r.throughputRps, rec.throughputCvPct),
                fmtMs(r.p50Ms),
                fmtMs(r.p90Ms),
                fmtMs(r.p99Ms),
                fmtMs(r.maxMs),
                fmtSig(cpuMsPerReq(r), 3),
                fmtMB(r.peakRssBytes),
                std::to_string(r.errors),
                notes
            }) << "\n";
        }
        out << "\n";

        // Speedup lines
        out << "### Speedup vs Legacy (" << transport << ")\n\n";

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

        for (unsigned conc : concLevels)
        {
            double legacyRps = 0.0, rustARps = 0.0, rustBRps = 0.0;
            for (auto const& rec : records)
            {
                if (rec.transport != transport || rec.concurrency != conc)
                    continue;
                if (rec.label == "Legacy")
                    legacyRps = rec.result.throughputRps;
                else if (rec.label == "Rust(reuse-on)")
                    rustARps = rec.result.throughputRps;
                else if (rec.label == "Rust(forced-close)")
                    rustBRps = rec.result.throughputRps;
            }
            if (legacyRps > 0.0)
            {
                out << "- c=" << conc
                    << ": Rust-A / Legacy = " << fmtSig(rustARps / legacyRps, 3) << "x"
                    << ";  Rust-B / Legacy = " << fmtSig(rustBRps / legacyRps, 3) << "x\n";
            }
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
    out << "- **Warmup excluded**: the first " << warmup << " requests of each batch are "
           "unmeasured and excluded from all statistics.\n";
    out << "- **Socket-drain wait**: each concurrency level runs repeated batches of "
        << requests << " requests per client with a " << waitSeconds << "s pause between "
           "batches so TIME_WAIT sockets are reclaimed; this keeps the benchmark from measuring "
           "socket-reclaim speed once a long sweep would otherwise exhaust ephemeral ports.\n";
    out << "- **Convergence**: batches repeat until throughput CV <= " << fmtPct(targetCvPct)
        << "% for every cell (>= " << minIterations << " samples) or " << maxIterations
        << " batches elapse. Rows that hit the cap first are flagged \"did not converge\" in "
           "Notes; reported throughput is still the mean of the batches taken.\n";
    out << "- **Fresh server per batch**: every cell run spins up its own BenchServer on a new "
           "ephemeral port, so each gets an isolated TCP 4-tuple space + fresh server threads.\n";
    out << "- **Driver watchdog**: each request has a backstop deadline; a request that "
           "doesn't complete in time is recorded as a timeout error and the loop proceeds "
           "(rather than hanging). Watchdog timeouts appear in the Errors column.\n";
    out << "- **Symmetric thread split**: each client runs N runtime threads + "
        << controlThreads << " control thread(s). Legacy = " << legacyThreads
        << " network io_context + " << controlThreads << " control = "
        << (legacyThreads + controlThreads) << "; Rust = " << tokioThreads
        << " Tokio + " << controlThreads << " control = " << (tokioThreads + controlThreads)
        << ". The runtime does network I/O; the control io_context fires requests and "
           "records stats, with completions hopped runtime->control in BOTH paths. "
           "Adjust `--legacy-threads`/`--tokio-threads`/`--control-threads` to taste.\n";
    out << "- **Measurements are wall-clock**: throughput is (ok+errors)/wallSeconds over "
           "the measured window; CPU ms/req is (user+sys CPU) / total requests.\n";
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
        ("requests",         po::value<unsigned>()->default_value(10000),
                             "measured requests per iteration, per client")
        ("warmup",           po::value<unsigned>()->default_value(0),
                             "unmeasured warmup requests per iteration, per client")
        ("concurrency",      po::value<std::string>()->default_value("8,64"),
                             "comma-separated concurrency levels to sweep")
        ("wait-seconds",     po::value<unsigned>()->default_value(70),
                             "seconds to wait between batches for sockets to free")
        ("target-cv",        po::value<double>()->default_value(1.0),
                             "stop a level once throughput CV (std-dev/mean, %) is at or below this")
        ("min-iterations",   po::value<unsigned>()->default_value(3),
                             "minimum batches per concurrency level before convergence can trigger")
        ("max-iterations",   po::value<unsigned>()->default_value(50),
                             "maximum batches per concurrency level (safety cap)")
        ("response-size",    po::value<std::size_t>()->default_value(1024),
                             "server response body size in bytes")
        ("transport",        po::value<std::string>()->default_value("both"),
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
    unsigned const waitSeconds       = vm["wait-seconds"].as<unsigned>();
    double const targetCvPct         = vm["target-cv"].as<double>();
    unsigned minIterations           = vm["min-iterations"].as<unsigned>();
    unsigned maxIterations           = vm["max-iterations"].as<unsigned>();
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

    // A coefficient of variation needs >= 2 samples; clamp so convergence is
    // meaningful and the cap can never sit below the floor.
    minIterations = std::max(2u, minIterations);
    maxIterations = std::max(minIterations, maxIterations);

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

    std::vector<std::string> activeTransports;
    if (doHttp)
        activeTransports.push_back("http");
    if (doHttps)
        activeTransports.push_back("https");

    std::string const timestamp = nowIso();

    // A fresh BenchServer is created per cell run inside the loop below, so each
    // run gets its own ephemeral port + server threads. `enableTls` only gates
    // whether https runs happen.

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

    // Run a single cell once: spin up a fresh server, drive `requests` requests,
    // and return the measured result.
    auto const runCell = [&](std::string const& transport, bool isTls, unsigned conc,
                             CellSpec const& cell) -> bench::RunResult
    {
        if (cell.client == bench::ClientKind::Rust)
            initRustTls(cell.disableReuse);

        bench::BenchServer server(serverThreads, responseSize, isTls);
        bench::RunConfig cfg;
        cfg.client                 = cell.client;
        cfg.tls                    = isTls;
        cfg.host                   = "127.0.0.1";
        cfg.port                   = server.port();
        cfg.path                   = "/bench";
        cfg.maxResponseBytes       = maxRespBytes;
        cfg.totalRequests          = requests;
        cfg.warmupRequests         = warmup;
        cfg.concurrency            = conc;
        cfg.ioThreads              = (cell.client == bench::ClientKind::Legacy)
            ? legacyThreads : tokioThreads;
        cfg.controlThreads         = controlThreads;
        cfg.timeout                = std::chrono::milliseconds(
            static_cast<std::chrono::milliseconds::rep>(timeoutSec) * 1000);
        cfg.requestConnectionClose = cell.requestConnectionClose;

        return (cell.client == bench::ClientKind::Legacy)
            ? bench::runLegacy(cfg)
            : bench::runRust(cfg);
    };

    // The cells driven each iteration: C++ legacy first, then the two Rust
    // cells. On loopback each fresh-connection request consumes 2 sockets
    // (client + server endpoint), so a batch of 3 cells x `requests` requests
    // uses up to 6 * requests sockets (~60k at the 10k default) before the
    // inter-batch wait reclaims them.
    std::vector<CellSpec> const cells = {
        {"Legacy",             bench::ClientKind::Legacy, /*disableReuse=*/false, /*close=*/false},
        {"Rust(reuse-on)",     bench::ClientKind::Rust,   /*disableReuse=*/false, /*close=*/false},
        {"Rust(forced-close)", bench::ClientKind::Rust,   /*disableReuse=*/true,  /*close=*/true},
    };

    // First batch overall runs immediately; every subsequent batch (including
    // the first batch of a new concurrency level / transport) waits first, so
    // TIME_WAIT sockets from the previous batch have drained.
    bool firstBatch = true;

    for (auto const& transport : activeTransports)
    {
        bool const isTls = (transport == "https");

        for (unsigned conc : concLevels)
        {
            // Per-cell throughput samples, accumulated across batches.
            std::vector<std::vector<bench::RunResult>> cellRuns(cells.size());
            unsigned iteration = 0;

            while (true)
            {
                if (!firstBatch)
                {
                    std::cerr << "waiting " << waitSeconds
                              << "s for sockets to free...\n";
                    std::this_thread::sleep_for(std::chrono::seconds(waitSeconds));
                }
                firstBatch = false;

                ++iteration;
                std::cerr << "=== " << transport << " c=" << conc
                          << " batch " << iteration << " ===\n";

                for (std::size_t ci = 0; ci < cells.size(); ++ci)
                {
                    auto const& cell = cells[ci];
                    std::cerr << "  " << cell.label << " ... " << std::flush;
                    bench::RunResult r = runCell(transport, isTls, conc, cell);
                    std::cerr << fmtRps(r.throughputRps) << " req/s";
                    if (r.errors)
                        std::cerr << " (" << r.errors << " errors)";
                    cellRuns[ci].push_back(std::move(r));

                    // Show the running CV so progress toward convergence is visible.
                    std::vector<double> t;
                    for (auto const& rr : cellRuns[ci])
                        t.push_back(rr.throughputRps);
                    if (t.size() >= 2)
                        std::cerr << "  [CV " << fmtPct(cvPct(t)) << "%]";
                    std::cerr << "\n";
                }

                // Converged once every cell's throughput CV is at or below the
                // target (with enough samples). Otherwise loop until the cap.
                bool allConverged = (iteration >= minIterations);
                if (allConverged)
                {
                    for (auto const& runs : cellRuns)
                    {
                        std::vector<double> t;
                        for (auto const& rr : runs)
                            t.push_back(rr.throughputRps);
                        if (cvPct(t) > targetCvPct)
                        {
                            allConverged = false;
                            break;
                        }
                    }
                }

                if (allConverged)
                {
                    std::cerr << "converged after " << iteration << " batches\n";
                    break;
                }
                if (iteration >= maxIterations)
                {
                    std::cerr << "reached max " << maxIterations
                              << " batches without converging\n";
                    break;
                }
            }

            for (std::size_t ci = 0; ci < cells.size(); ++ci)
            {
                RunRecord rec = aggregate(
                    transport, conc, cells[ci].label, cellRuns[ci],
                    targetCvPct, minIterations);
                std::cerr << "  " << rec.label << ": "
                          << fmtRpsCv(rec.result.throughputRps, rec.throughputCvPct)
                          << " over " << rec.iterations << " batches"
                          << (rec.converged ? "" : " (did not converge)") << "\n";
                allRecords.push_back(std::move(rec));
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
            waitSeconds,
            targetCvPct,
            minIterations,
            maxIterations,
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
