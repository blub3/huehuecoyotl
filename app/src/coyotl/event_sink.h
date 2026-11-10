#pragma once
// The one C++ function the Rust core calls (SPEC 8, rule 2). Included by the cxx-generated glue.
#include <functional>

class QObject;

namespace coyotl::core {

struct CoreEvent;

class EventSink {
  public:
    using Handler = std::function<void(const CoreEvent &)>;
    // `handler` runs on the thread of `context` (the Qt GUI thread) via a queued invocation.
    EventSink(QObject *context, Handler handler);

    // Thread-safe; called from the core's worker threads. Copies the event and returns.
    void post(const CoreEvent &event) const;

  private:
    QObject *m_context;
    Handler m_handler;
};

} // namespace coyotl::core
