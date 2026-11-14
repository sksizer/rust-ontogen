import Foundation
import IronLogCore
import Observation

/// The app's view of the log. The same code runs on either backend: which one
/// is decided by the static library the app was linked against.
@MainActor
@Observable
final class TrainingLog {
    private(set) var sessions: [Session] = []
    private(set) var detail: SessionDetail?
    private(set) var exercises: [Exercise] = []
    private(set) var muscles: [Muscle] = []
    private(set) var contributions: [ExerciseMuscle] = []
    private(set) var volumes: [MuscleVolume] = []
    private(set) var bests: [BestSet] = []
    private(set) var lastChange: String?
    var error: String?
    var selection: String? {
        didSet { Task { await reloadDetail() } }
    }

    let backend = backendName()
    let dataPath: String
    private let api: NativeApi?

    static let personId = "me"
    static let strengthKind = "strength"

    init(dataPath: String) {
        self.dataPath = dataPath
        do {
            api = try openIronLog(dataDir: dataPath)
        } catch {
            api = nil
            self.error = "\(error)"
        }
    }

    /// Seed an empty log, load it, then reload on every change the core
    /// reports. Runs for the life of the window.
    func run() async {
        guard let api else { return }
        await attempt { try await api.trainingSeedCatalog(now: Self.now()) }
        await reload()
        do {
            for await frame in try await api.entityChangesSubscribe().frames {
                switch frame {
                case .event(_, let change):
                    lastChange = "\(change.op) \(change.kind) \(change.id)"
                case .lag(let skipped):
                    lastChange = "missed \(skipped) changes"
                }
                await reload()
            }
        } catch {
            self.error = "\(error)"
        }
    }

    func reload() async {
        guard let api else { return }
        await attempt {
            sessions = try await api.sessionList(sort: ["-started_at"])
            exercises = try await api.exerciseList(sort: ["name"])
            muscles = try await api.muscleList(sort: ["name"])
            contributions = try await api.exerciseMuscleList(sort: nil)
            volumes = try await api.trainingVolumeByMuscle(personId: Self.personId, since: "2000")
            bests = try await api.trainingBestSets(personId: Self.personId)
        }
        await reloadDetail()
    }

    private func reloadDetail() async {
        guard let api, let id = selection, sessions.contains(where: { $0.id == id }) else {
            detail = nil
            return
        }
        await attempt { detail = try await api.trainingSessionDetail(id: id) }
    }

    /// A new session starting now; returns its id.
    func startSession(title: String) async -> String? {
        guard let api else { return nil }
        let input = CreateSessionInput(
            id: Self.newId("s"), personId: Self.personId, activityKindId: Self.strengthKind,
            locationId: "home-gym", title: title, startedAt: Self.now(), endedAt: nil, tags: [], notes: nil)
        return await attempt { try await api.sessionCreate(input: input).id }
    }

    func logSet(session: String, exercise: String, intent: Intent, kilos: Double?, reps: Int32, rpe: Double?) async {
        guard let api else { return }
        let input = LogSetInput(
            id: Self.newId("e"), personId: Self.personId, sessionId: session, exerciseId: exercise,
            kindId: Self.strengthKind, startedAt: Self.now(),
            position: Int32((detail?.sets.count ?? 0) + 1), intent: intent,
            loadGrams: kilos.map { Int32(($0 * 1000).rounded()) }, reps: reps, rpe: rpe)
        await attempt { _ = try await api.trainingLogSet(input: input) }
    }

    /// `.some(nil)` clears the end time, reopening the session.
    func setEnded(_ id: String, at time: String?) async {
        guard let api else { return }
        let patch = UpdateSessionInput(
            personId: nil, activityKindId: nil, locationId: nil, title: nil, startedAt: nil,
            endedAt: .some(time), tags: nil, notes: nil)
        await attempt { _ = try await api.sessionUpdate(id: id, input: patch) }
    }

    func delete(session id: String) async {
        guard let api else { return }
        await attempt { try await api.trainingDeleteSessionCascade(id: id) }
    }

    static func now() -> String { Date.now.formatted(.iso8601) }

    private static func newId(_ prefix: String) -> String {
        "\(prefix)-\(UUID().uuidString.prefix(8).lowercased())"
    }

    @discardableResult
    private func attempt<T>(_ body: () async throws -> T) async -> T? {
        do {
            let value = try await body()
            error = nil
            return value
        } catch ApiError.Failed(let code, let message) {
            error = "\(message) (\(code))"
        } catch {
            self.error = "\(error)"
        }
        return nil
    }
}
