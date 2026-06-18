#include <xrpld/rpc/RPCSub.h>

#include <xrpld/rpc/RPCCall.h>

#include <xrpl/basics/Log.h>
#include <xrpl/basics/StringUtilities.h>
#include <xrpl/basics/contract.h>
#include <xrpl/core/Job.h>
#include <xrpl/core/JobQueue.h>
#include <xrpl/core/ServiceRegistry.h>
#include <xrpl/json/json_value.h>
#include <xrpl/json/to_string.h>  // IWYU pragma: keep
#include <xrpl/server/InfoSub.h>

#include <boost/asio/io_context.hpp>

#include <deque>
#include <exception>
#include <functional>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <utility>

namespace xrpl {

// Subscription object for JSON-RPC
class RPCSubImp : public RPCSub
{
public:
    RPCSubImp(
        InfoSub::Source& source,
        boost::asio::io_context& ioContext,
        JobQueue& jobQueue,
        std::string const& strUrl,
        std::string strUsername,
        std::string strPassword,
        ServiceRegistry& registry)
        : RPCSub(source)
        , ioContext_(ioContext)
        , jobQueue_(jobQueue)
        , url_(strUrl)
        , username_(std::move(strUsername))
        , password_(std::move(strPassword))
        , j_(registry.getJournal("RPCSub"))
        , logs_(registry.getLogs())
    {
        ParsedUrl pUrl;

        if (!parseUrl(pUrl, strUrl))
            Throw<std::runtime_error>("Failed to parse url.");

        if (pUrl.scheme != "http" && pUrl.scheme != "https")
            Throw<std::runtime_error>("Only http and https is supported.");

        seq_ = 1;

        JLOG(j_.info()) << "RPCCall::fromNetwork sub: url='" << url_ << "'";
    }

    ~RPCSubImp() override = default;

    void
    send(json::Value const& jvObj, bool broadcast) override
    {
        std::scoped_lock const sl(lock_);

        auto jm = broadcast ? j_.debug() : j_.info();
        JLOG(jm) << "RPCCall::fromNetwork push: " << jvObj;

        deque_.emplace_back(seq_++, jvObj);

        if (!sending_)
        {
            // Start a sending thread.
            JLOG(j_.info()) << "RPCCall::fromNetwork start";

            sending_ =
                jobQueue_.addJob(JtClientSubscribe, "RPCSubSendThr", [this]() { sendThread(); });
        }
    }

    void
    setUsername(std::string const& strUsername) override
    {
        std::scoped_lock const sl(lock_);

        username_ = strUsername;
    }

    void
    setPassword(std::string const& strPassword) override
    {
        std::scoped_lock const sl(lock_);

        password_ = strPassword;
    }

private:
    // XXX Could probably create a bunch of send jobs in a single get of the
    // lock.
    void
    sendThread()
    {
        json::Value jvEvent;
        bool bSend = false;

        do
        {
            {
                // Obtain the lock to manipulate the queue and change sending.
                std::scoped_lock const sl(lock_);

                if (deque_.empty())
                {
                    sending_ = false;
                    bSend = false;
                }
                else
                {
                    auto const [seq, env] = deque_.front();

                    deque_.pop_front();

                    jvEvent = env;
                    jvEvent["seq"] = seq;

                    bSend = true;
                }
            }

            // Send outside of the lock.
            if (bSend)
            {
                // XXX Might not need this in a try.
                try
                {
                    JLOG(j_.info()) << "RPCCall::fromNetwork: " << url_;

                    RPCCall::fromNetwork(
                        ioContext_,
                        url_,
                        username_,
                        password_,
                        "event",
                        jvEvent,
                        true,
                        logs_);
                }
                catch (std::exception const& e)
                {
                    JLOG(j_.info()) << "RPCCall::fromNetwork exception: " << e.what();
                }
            }
        } while (bSend);
    }

private:
    boost::asio::io_context& ioContext_;
    JobQueue& jobQueue_;

    std::string url_;
    std::string username_;
    std::string password_;

    int seq_;  // Next id to allocate.

    bool sending_{false};  // Sending thread is active.

    std::deque<std::pair<int, json::Value>> deque_;

    beast::Journal const j_;
    Logs& logs_;
};

//------------------------------------------------------------------------------

RPCSub::RPCSub(InfoSub::Source& source) : InfoSub(source, Consumer())
{
}

std::shared_ptr<RPCSub>
makeRPCSub(
    InfoSub::Source& source,
    boost::asio::io_context& ioContext,
    JobQueue& jobQueue,
    std::string const& strUrl,
    std::string const& strUsername,
    std::string const& strPassword,
    ServiceRegistry& registry)
{
    return std::make_shared<RPCSubImp>(
        std::ref(source),
        std::ref(ioContext),
        std::ref(jobQueue),
        strUrl,
        strUsername,
        strPassword,
        registry);
}

}  // namespace xrpl
