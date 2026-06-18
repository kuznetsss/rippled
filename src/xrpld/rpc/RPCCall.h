#pragma once

#include <xrpld/core/Config.h>

#include <xrpl/core/ServiceRegistry.h>
#include <xrpl/json/json_value.h>

#include <boost/asio/io_context.hpp>

#include <functional>
#include <string>
#include <unordered_map>
#include <vector>

namespace xrpl {

// This a trusted interface, the user is expected to provide valid input to
// perform valid requests. Error catching and reporting is not a requirement of
// the command line interface.
//
// Improvements to be more strict and to provide better diagnostics are welcome.

/** Processes XRPL RPC calls. */
namespace RPCCall {

int
fromCommandLine(Config const& config, std::vector<std::string> const& vCmd, Logs& logs);

/**
 * @brief Send a JSON-RPC request to @p strUrl and deliver the parsed reply.
 *
 * Issues an HTTP POST carrying the JSON-RPC method and params through the
 * global HTTP client (see initHTTPClient()), completing on @p ioContext. The
 * call is asynchronous; run @p ioContext to drive it to completion.
 *
 * @param ioContext io_context whose executor receives the completion
 * @param strUrl endpoint URL (http or https)
 * @param strUsername HTTP basic-auth username
 * @param strPassword HTTP basic-auth password
 * @param strMethod JSON-RPC method name
 * @param jvParams JSON-RPC parameters
 * @param quiet suppress the informational "connecting" log line
 * @param logs log sink
 * @param callbackFuncP invoked with the parsed JSON reply, if set
 * @param headers extra HTTP headers to send with the request
 */
void
fromNetwork(
    boost::asio::io_context& ioContext,
    std::string const& strUrl,
    std::string const& strUsername,
    std::string const& strPassword,
    std::string const& strMethod,
    json::Value const& jvParams,
    bool quiet,
    Logs& logs,
    std::function<void(json::Value const& jvInput)> callbackFuncP =
        std::function<void(json::Value const& jvInput)>(),
    std::unordered_map<std::string, std::string> headers = {});
}  // namespace RPCCall

json::Value
rpcCmdToJson(
    std::vector<std::string> const& args,
    json::Value& retParams,
    unsigned int apiVersion,
    beast::Journal j);

/** Internal invocation of RPC client.
 *  Used by both xrpld command line as well as xrpld unit tests
 */
std::pair<int, json::Value>
rpcClient(
    std::vector<std::string> const& args,
    Config const& config,
    Logs& logs,
    unsigned int apiVersion,
    std::unordered_map<std::string, std::string> const& headers = {});

}  // namespace xrpl
