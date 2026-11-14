import Foundation
import Testing

@testable import IronLogCore

/// The Swift half of the shared scenario: it runs against whichever backend
/// the library was built with, so `scripts/test.sh` runs it twice.

private func freshLog() async throws -> NativeApi {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("iron-log-mac-\(UUID().uuidString)")
    let api = try openIronLog(dataDir: dir.path)
    try await api.trainingSeedCatalog(now: "2026-01-01T00:00:00Z")
    return api
}

private func sessionInput(_ id: String, tags: [String] = []) -> CreateSessionInput {
    CreateSessionInput(
        id: id, personId: "me", activityKindId: "strength", locationId: "home-gym", title: "Push day",
        startedAt: "2026-02-01T10:00:00Z", endedAt: "2026-02-01T11:00:00Z", tags: tags, notes: nil)
}

private func setInput(_ id: String, session: String?, intent: Intent, grams: Int32, reps: Int32) -> LogSetInput {
    LogSetInput(
        id: id, personId: "me", sessionId: session, exerciseId: "bench-press", kindId: "strength",
        startedAt: "2026-02-01T10:30:00Z", position: 1, intent: intent, loadGrams: grams, reps: reps, rpe: 8.5)
}

private func failureCode(_ body: () async throws -> Void) async -> String? {
    do {
        try await body()
        return nil
    } catch ApiError.Failed(let code, _) {
        return code
    } catch {
        return "\(error)"
    }
}

@Test func theCatalogCrossesTheFfiWithItsRelations() async throws {
    let api = try await freshLog()
    let pec = try await api.muscleGetById(id: "pec-major")
    #expect(pec.heads == ["pec-clavicular", "pec-sternal"])
    #expect(pec.groups.sorted() == ["chest", "push"])
    #expect(pec.region == .upper)
    let bench = try await api.exerciseGetById(id: "bench-press")
    #expect(bench.cues == [Cue(text: "Shoulder blades pinned", phase: nil)])
    #expect(bench.mechanic == .compound)
}

@Test func aSessionRollsUpAndCascades() async throws {
    let api = try await freshLog()
    _ = try await api.sessionCreate(input: sessionInput("s1", tags: ["heavy"]))
    _ = try await api.trainingLogSet(input: setInput("e1", session: "s1", intent: .warmup, grams: 40_000, reps: 10))
    let line = try await api.trainingLogSet(input: setInput("e2", session: "s1", intent: .working, grams: 100_000, reps: 5))
    #expect(line.e1rmGrams == 116_667)

    let detail = try await api.trainingSessionDetail(id: "s1")
    #expect(detail.tagNames == ["Heavy"])
    #expect(detail.locationName == "Home Gym")
    #expect(detail.sets.count == 2)

    let volumes = try await api.trainingVolumeByMuscle(personId: "me", since: "2026")
    #expect(volumes.first?.muscleId == "pec-major")
    #expect(volumes.first?.volumeKg == 300)

    let bests = try await api.trainingBestSets(personId: "me")
    #expect(bests.first?.loadGrams == 100_000)

    try await api.trainingDeleteSessionCascade(id: "s1")
    #expect(try await api.sessionList(sort: nil).isEmpty)
    #expect(try await api.effortList(sort: nil).isEmpty)
}

@Test func aDoubleOptionalClearsOrLeavesAlone() async throws {
    let api = try await freshLog()
    _ = try await api.sessionCreate(input: sessionInput("s1"))

    // `String??`: `.some(nil)` clears the field, `nil` leaves it as it is.
    let updated = try await api.sessionUpdate(
        id: "s1",
        input: UpdateSessionInput(
            personId: nil, activityKindId: nil, locationId: .some(nil), title: nil, startedAt: nil,
            endedAt: .some(nil), tags: nil, notes: .some("edited")))
    #expect(updated.locationId == nil)
    #expect(updated.endedAt == nil)
    #expect(updated.title == "Push day")
    #expect(updated.notes == "edited")
}

@Test func errorsCarryTheirWireCode() async throws {
    let api = try await freshLog()
    #expect(await failureCode { _ = try await api.sessionGetById(id: "nope") } == "session_not_found")
    #expect(await failureCode { _ = try await api.sessionList(sort: ["bogus"]) } == "invalid_sort_field")
    #expect(
        await failureCode {
            _ = try await api.trainingLogSet(input: setInput("e1", session: nil, intent: .working, grams: 1000, reps: 0))
        } == "invalid_log")
}

/// The one place the two backends differ on purpose, seen from Swift.
@Test func deletingWhatIsReferencedDependsOnTheBackend() async throws {
    let api = try await freshLog()
    _ = try await api.trainingLogSet(input: setInput("e1", session: nil, intent: .working, grams: 50_000, reps: 5))
    let code = await failureCode { try await api.exerciseDelete(id: "bench-press") }
    switch backendName() {
    case "SQLite": #expect(code == "db_error")
    case "Markdown": #expect(code == nil)
    default: Issue.record("unknown backend \(backendName())")
    }
}

@Test func theChangeFeedReportsWrites() async throws {
    let api = try await freshLog()
    let feed = try await api.entityChangesSubscribe()

    _ = try await api.tagCreate(input: CreateTagInput(id: "cardio", name: "Cardio"))
    try await api.tagDelete(id: "cardio")

    var seen: [String] = []
    for await frame in feed.frames {
        if case .event(_, let change) = frame {
            seen.append("\(change.op) \(change.kind) \(change.id)")
        }
        if seen.count == 2 { break }
    }
    #expect(seen == ["created tag cardio", "deleted tag cardio"])
}
