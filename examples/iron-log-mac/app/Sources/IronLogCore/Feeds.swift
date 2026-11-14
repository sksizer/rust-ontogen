// Hand-written: UniFFI has no stream type, so an ontogen event op arrives as
// a subscription object with `next()`. This adapts it to Swift concurrency.

extension EntityChangesSubscription {
    /// The feed as an `AsyncStream`, ending when the Rust feed closes.
    ///
    /// Ending the stream stops pulling, but a `next()` already in flight
    /// stays pending in Rust until the next change arrives: UniFFI does not
    /// carry Swift task cancellation into the Rust future.
    public var frames: AsyncStream<EntityChangeFrame> {
        AsyncStream { continuation in
            let task = Task {
                while !Task.isCancelled, let frame = await self.next() {
                    continuation.yield(frame)
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
}
