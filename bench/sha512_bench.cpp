// sha512_bench.cpp
//
// Micro-benchmark for `sha512Half` (the first 256 bits of a SHA-512 digest),
// which is the dominant hashing primitive in rippled. This translation unit is
// deliberately backend-agnostic: it includes <openssl/sha.h> and calls the
// legacy SHA512_Init/Update/Final API exactly as src/libxrpl/protocol/digest.cpp
// does. The CMake build compiles it twice -- once linked against the project's
// OpenSSL and once against aws-lc -- so the two binaries can be compared.
//
// The workload mirrors how rippled actually hashes: many small messages built
// from several segments fed through separate Update() calls (e.g. a SHAMap
// inner node is a 4-byte prefix followed by up to sixteen 32-byte child
// hashes), not one giant buffer. A weighted "ledger mix" approximates the
// relative frequency of those shapes during a ledger close.

#define OPENSSL_SUPPRESS_DEPRECATED 1  // SHA512_* are deprecated in OpenSSL 3.x

#include <openssl/crypto.h>
#include <openssl/sha.h>

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <functional>
#include <random>
#include <string>
#include <vector>

namespace {

#ifndef BENCH_BACKEND
#define BENCH_BACKEND "unknown"
#endif

// Sink for digest bytes so the optimizer cannot elide the hashing work.
volatile std::uint8_t g_sink = 0;

// A message is a backing byte store plus the segment boundaries that get fed to
// SHA512_Update one at a time, replicating the codebase's hash_append pattern.
struct Message
{
    std::vector<std::uint8_t> bytes;
    std::vector<std::pair<std::size_t, std::size_t>> segs;  // (offset, length)
    std::size_t total = 0;
};

void
addSeg(Message& m, std::mt19937& rng, std::size_t len)
{
    std::size_t const off = m.bytes.size();
    m.bytes.resize(off + len);
    std::uniform_int_distribution<int> d(0, 255);
    for (std::size_t i = 0; i < len; ++i)
        m.bytes[off + i] = static_cast<std::uint8_t>(d(rng));
    m.segs.emplace_back(off, len);
    m.total += len;
}

inline void
hashMessage(Message const& m, std::uint8_t out[SHA512_DIGEST_LENGTH])
{
    SHA512_CTX ctx;
    SHA512_Init(&ctx);
    for (auto const& s : m.segs)
        SHA512_Update(&ctx, m.bytes.data() + s.first, s.second);
    SHA512_Final(out, &ctx);  // 64 bytes; sha512Half would keep the first 32
}

// ---- Workload generators (one Message each). ------------------------------
// Sizes/segment counts are taken from real sha512Half call sites in libxrpl.

constexpr std::size_t kPrefix = 4;   // HashPrefix (4-byte big-endian tag)
constexpr std::size_t kHash = 32;    // a 256-bit child hash / account key

// SHAMapInnerNode::updateHash: prefix + 16 child hashes, 17 Update() calls.
Message
genInnerNodeFull(std::mt19937& rng)
{
    Message m;
    addSeg(m, rng, kPrefix);
    for (int i = 0; i < 16; ++i)
        addSeg(m, rng, kHash);
    return m;
}

// A typical sparser inner node (only a few populated children).
Message
genInnerNodeSparse(std::mt19937& rng)
{
    Message m;
    addSeg(m, rng, kPrefix);
    std::uniform_int_distribution<int> nChildren(2, 6);
    int const n = nChildren(rng);
    for (int i = 0; i < n; ++i)
        addSeg(m, rng, kHash);
    return m;
}

// Transaction id: prefix + serialized transaction blob.
Message
genTxId(std::mt19937& rng)
{
    Message m;
    addSeg(m, rng, kPrefix);
    std::uniform_int_distribution<int> body(120, 400);
    addSeg(m, rng, static_cast<std::size_t>(body(rng)));
    return m;
}

// Tx+meta leaf (SHAMapTxPlusMetaLeafNode): prefix + blob + 32-byte key.
Message
genTxMetaLeaf(std::mt19937& rng)
{
    Message m;
    addSeg(m, rng, kPrefix);
    std::uniform_int_distribution<int> blob(150, 700);
    addSeg(m, rng, static_cast<std::size_t>(blob(rng)));
    addSeg(m, rng, kHash);
    return m;
}

// Small single-segment hash (e.g. account index / ledger key material).
Message
genAcctKey(std::mt19937& rng)
{
    Message m;
    addSeg(m, rng, kHash);
    return m;
}

// The original "from the past" test: one big 100 KB buffer (streaming).
Message
genBigBuffer(std::mt19937& rng)
{
    Message m;
    addSeg(m, rng, 100000);
    return m;
}

std::vector<Message>
buildPool(std::function<Message(std::mt19937&)> const& gen, std::size_t n, std::mt19937& rng)
{
    std::vector<Message> pool;
    pool.reserve(n);
    for (std::size_t i = 0; i < n; ++i)
        pool.push_back(gen(rng));
    return pool;
}

// ---- Measurement ----------------------------------------------------------

struct Result
{
    std::size_t avgBytes = 0;
    double nsPerHash = 0;
    double mHashPerSec = 0;
    double mbPerSec = 0;
};

Result
measurePool(std::vector<Message> const& pool, int trials, std::chrono::milliseconds budget)
{
    using clock = std::chrono::steady_clock;

    std::size_t totalBytes = 0;
    for (auto const& m : pool)
        totalBytes += m.total;
    double const avgBytes = static_cast<double>(totalBytes) / pool.size();

    std::uint8_t out[SHA512_DIGEST_LENGTH];

    // Warm up caches / branch predictors on the whole pool.
    for (auto const& m : pool)
    {
        hashMessage(m, out);
        g_sink ^= out[0];
    }

    constexpr int kBatch = 64;  // hashes per clock read, to amortize timing
    std::vector<double> samples;
    samples.reserve(trials);

    for (int t = 0; t < trials; ++t)
    {
        std::uint64_t count = 0;
        std::size_t idx = 0;
        auto const t0 = clock::now();
        clock::duration elapsed{};
        do
        {
            for (int b = 0; b < kBatch; ++b)
            {
                hashMessage(pool[idx], out);
                g_sink ^= out[0];
                if (++idx == pool.size())
                    idx = 0;
            }
            count += kBatch;
            elapsed = clock::now() - t0;
        } while (elapsed < budget);

        double const ns =
            static_cast<double>(std::chrono::duration_cast<std::chrono::nanoseconds>(elapsed).count());
        samples.push_back(ns / static_cast<double>(count));
    }

    std::sort(samples.begin(), samples.end());
    double const median = samples[samples.size() / 2];

    Result r;
    r.avgBytes = static_cast<std::size_t>(avgBytes + 0.5);
    r.nsPerHash = median;
    r.mHashPerSec = 1000.0 / median;            // (1e9 / median) / 1e6
    r.mbPerSec = (avgBytes / median) * 1000.0;  // bytes/ns -> MB/s
    return r;
}

struct Workload
{
    char const* name;
    std::function<Message(std::mt19937&)> gen;
    std::size_t poolSize;
    int mixWeight;  // relative frequency in the "ledger mix" (0 = excluded)
};

}  // namespace

int
main(int argc, char** argv)
{
    bool const csv = (argc > 1 && std::string(argv[1]) == "--csv");

    char const* const backend = BENCH_BACKEND;
    char const* const version = OpenSSL_version(OPENSSL_VERSION);

    // Fixed seed: identical corpus across runs and across backends.
    std::mt19937 rng(0xC0FFEEu);

    std::vector<Workload> const workloads = {
        {"inner_node_full", genInnerNodeFull, 512, 50},
        {"inner_node_sparse", genInnerNodeSparse, 512, 20},
        {"tx_meta_leaf", genTxMetaLeaf, 512, 15},
        {"tx_id", genTxId, 512, 10},
        {"acct_key", genAcctKey, 1024, 5},
        {"big_buffer_100k", genBigBuffer, 64, 0},  // streaming, not in the mix
    };

    int const trials = 7;
    auto const budget = std::chrono::milliseconds(120);

    struct Row
    {
        std::string name;
        Result res;
    };
    std::vector<Row> rows;

    // Build a weighted "ledger mix" pool from the in-mix generators.
    std::vector<Message> mixPool;
    {
        constexpr std::size_t kMixSize = 4096;
        int weightSum = 0;
        for (auto const& w : workloads)
            weightSum += w.mixWeight;
        std::uniform_int_distribution<int> pick(1, weightSum);
        mixPool.reserve(kMixSize);
        for (std::size_t i = 0; i < kMixSize; ++i)
        {
            int roll = pick(rng);
            for (auto const& w : workloads)
            {
                if (w.mixWeight == 0)
                    continue;
                roll -= w.mixWeight;
                if (roll <= 0)
                {
                    mixPool.push_back(w.gen(rng));
                    break;
                }
            }
        }
    }

    for (auto const& w : workloads)
    {
        auto pool = buildPool(w.gen, w.poolSize, rng);
        rows.push_back({w.name, measurePool(pool, trials, budget)});
    }
    // Insert the mix right after the per-shape rows.
    rows.push_back({"ledger_mix", measurePool(mixPool, trials, budget)});

    if (csv)
    {
        // backend,version,workload,avg_bytes,ns_per_hash,mhash_per_s,mb_per_s
        for (auto const& r : rows)
            std::printf(
                "%s,%s,%s,%zu,%.3f,%.4f,%.1f\n",
                backend,
                version,
                r.name.c_str(),
                r.res.avgBytes,
                r.res.nsPerHash,
                r.res.mHashPerSec,
                r.res.mbPerSec);
        return 0;
    }

    std::printf("backend : %s\n", backend);
    std::printf("version : %s\n", version);
    std::printf("trials  : %d x %lld ms (median reported)\n", trials, (long long)budget.count());
    std::printf("\n");
    std::printf(
        "%-20s %10s %12s %14s %12s\n",
        "workload",
        "bytes",
        "ns/hash",
        "Mhash/s",
        "MB/s");
    std::printf("%s\n", std::string(72, '-').c_str());
    for (auto const& r : rows)
        std::printf(
            "%-20s %10zu %12.1f %14.2f %12.0f\n",
            r.name.c_str(),
            r.res.avgBytes,
            r.res.nsPerHash,
            r.res.mHashPerSec,
            r.res.mbPerSec);

    return 0;
}
