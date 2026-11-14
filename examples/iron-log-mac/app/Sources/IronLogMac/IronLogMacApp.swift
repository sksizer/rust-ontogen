import AppKit
import Charts
import IronLogCore
import SwiftUI

@main
struct IronLogMacApp: App {
    @State private var log = TrainingLog(dataPath: IronLogMacApp.dataPath())

    init() {
        // Launched by `swift run` there is no bundle to make this a regular
        // app; without this the window opens behind the terminal.
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate()
    }

    var body: some Scene {
        WindowGroup("Iron Log — \(backendName())") {
            ContentView()
                .environment(log)
                .task { await log.run() }
        }
    }

    /// `IRON_LOG_DATA` when set, else a per-backend Application Support folder,
    /// so the two builds never share data.
    static func dataPath() -> String {
        if let path = ProcessInfo.processInfo.environment["IRON_LOG_DATA"] {
            return path
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("IronLog\(backendName())").path
    }
}

enum Pane: String, CaseIterable, Identifiable {
    case sessions = "Sessions"
    case progress = "Progress"
    case catalog = "Catalog"
    var id: Self { self }
}

struct ContentView: View {
    @Environment(TrainingLog.self) private var log
    @State private var pane = Pane.sessions

    var body: some View {
        Group {
            switch pane {
            case .sessions: SessionsPane()
            case .progress: ProgressPane()
            case .catalog: CatalogPane()
            }
        }
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("View", selection: $pane) {
                    ForEach(Pane.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
            }
        }
        .safeAreaInset(edge: .bottom) { StatusBar() }
    }
}

// MARK: Sessions

struct SessionsPane: View {
    @Environment(TrainingLog.self) private var log

    var body: some View {
        @Bindable var log = log
        NavigationSplitView {
            List(log.sessions, id: \.id, selection: $log.selection) { session in
                VStack(alignment: .leading) {
                    Text(session.title ?? "Untitled").font(.headline)
                    Text(session.startedAt).font(.caption).foregroundStyle(.secondary)
                }
                .contextMenu {
                    Button("Delete", role: .destructive) { Task { await log.delete(session: session.id) } }
                }
            }
            .navigationSplitViewColumnWidth(min: 220, ideal: 260)
            .toolbar {
                Button("New Session", systemImage: "plus") {
                    Task { log.selection = await log.startSession(title: "Training") }
                }
            }
        } detail: {
            if let detail = log.detail {
                SessionView(detail: detail).id(detail.session.id)
            } else {
                ContentUnavailableView("No session selected", systemImage: "dumbbell")
            }
        }
    }
}

struct SessionView: View {
    @Environment(TrainingLog.self) private var log
    let detail: SessionDetail

    var body: some View {
        Form {
            Section(detail.session.title ?? "Untitled") {
                LabeledContent("Started", value: detail.session.startedAt)
                LabeledContent("Ended", value: detail.session.endedAt ?? "In progress")
                LabeledContent("Where", value: detail.locationName ?? "—")
                LabeledContent("Tags", value: detail.tagNames.joined(separator: ", "))
                HStack {
                    Spacer()
                    if detail.session.endedAt == nil {
                        Button("Finish") { Task { await log.setEnded(detail.session.id, at: TrainingLog.now()) } }
                    } else {
                        Button("Reopen") { Task { await log.setEnded(detail.session.id, at: nil) } }
                    }
                    Button("Delete", role: .destructive) { Task { await log.delete(session: detail.session.id) } }
                }
            }
            Section("Sets") {
                ForEach(detail.sets, id: \.effortId) { SetRow(line: $0) }
                if detail.sets.isEmpty {
                    Text("Nothing logged yet.").foregroundStyle(.secondary)
                }
            }
            Section("Log a set") {
                LogSetForm(sessionId: detail.session.id)
            }
        }
        .formStyle(.grouped)
    }
}

struct SetRow: View {
    let line: SetLine

    var body: some View {
        HStack {
            Text(line.exerciseName)
            if line.intent == .warmup {
                Text("warmup").font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Text(line.loadGrams.map { "\(kilos($0)) kg × \(line.reps)" } ?? "\(line.reps) reps").monospacedDigit()
            if let e1rm = line.e1rmGrams {
                Text("e1RM \(kilos(e1rm))").font(.caption).foregroundStyle(.secondary).monospacedDigit()
            }
        }
    }
}

struct LogSetForm: View {
    @Environment(TrainingLog.self) private var log
    let sessionId: String
    @State private var exercise = ""
    @State private var intent = Intent.working
    @State private var load = ""
    @State private var reps = "5"
    @State private var rpe = ""

    var body: some View {
        Picker("Exercise", selection: $exercise) {
            ForEach(log.exercises, id: \.id) { Text($0.name).tag($0.id) }
        }
        Picker("Intent", selection: $intent) {
            Text("Warmup").tag(Intent.warmup)
            Text("Working").tag(Intent.working)
            Text("Attempt").tag(Intent.attempt)
        }
        TextField("Load (kg)", text: $load)
        TextField("Reps", text: $reps)
        TextField("RPE", text: $rpe)
        HStack {
            Spacer()
            Button("Log Set") {
                Task {
                    await log.logSet(
                        session: sessionId, exercise: exercise, intent: intent, kilos: Double(load),
                        reps: Int32(reps) ?? 0, rpe: Double(rpe))
                }
            }
            .keyboardShortcut(.return)
            .buttonStyle(.borderedProminent)
            .disabled(exercise.isEmpty)
        }
        .onAppear {
            if exercise.isEmpty { exercise = log.exercises.first?.id ?? "" }
        }
    }
}

// MARK: Progress

struct ProgressPane: View {
    @Environment(TrainingLog.self) private var log

    var body: some View {
        if log.volumes.isEmpty && log.bests.isEmpty {
            ContentUnavailableView("Log a working set to see progress", systemImage: "chart.bar")
        } else {
            Form {
                Section("Volume by muscle (kg, warmups excluded)") {
                    Chart(log.volumes, id: \.muscleId) { volume in
                        BarMark(x: .value("Volume", volume.volumeKg), y: .value("Muscle", volume.muscleName))
                    }
                    .frame(height: max(160, CGFloat(log.volumes.count) * 28))
                }
                Section("Best sets") {
                    ForEach(log.bests, id: \.exerciseId) { best in
                        LabeledContent(best.exerciseName) {
                            Text("\(kilos(best.loadGrams)) kg × \(best.reps), e1RM \(kilos(best.e1rmGrams))")
                                .monospacedDigit()
                        }
                    }
                }
            }
            .formStyle(.grouped)
        }
    }
}

// MARK: Catalog

struct CatalogPane: View {
    @Environment(TrainingLog.self) private var log

    var body: some View {
        Form {
            Section("Exercises") {
                ForEach(log.exercises, id: \.id) { exercise in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(exercise.name).font(.headline)
                        Text(worked(by: exercise)).font(.caption).foregroundStyle(.secondary)
                        ForEach(exercise.cues, id: \.text) { Text("“\($0.text)”").font(.caption).italic() }
                    }
                }
            }
            Section("Muscles") {
                ForEach(log.muscles.filter { $0.parentId == nil }, id: \.id) { muscle in
                    LabeledContent(muscle.name) {
                        Text(heads(of: muscle) + muscle.groups.joined(separator: ", ")).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private func worked(by exercise: Exercise) -> String {
        log.contributions
            .filter { $0.exerciseId == exercise.id }
            .sorted { $0.contribution > $1.contribution }
            .map { row in
                let name = log.muscles.first { $0.id == row.muscleId }?.name ?? row.muscleId
                return "\(name) \(Int((row.contribution * 100).rounded()))%"
            }
            .joined(separator: ", ")
    }

    private func heads(of muscle: Muscle) -> String {
        muscle.heads.isEmpty ? "" : "\(muscle.heads.count) heads · "
    }
}

// MARK: Shared

struct StatusBar: View {
    @Environment(TrainingLog.self) private var log

    var body: some View {
        HStack {
            Text(log.backend).bold()
            if let error = log.error {
                Label(error, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
            } else if let change = log.lastChange {
                Label("Live: \(change)", systemImage: "dot.radiowaves.left.and.right")
            }
            Spacer()
            Text(log.dataPath).truncationMode(.middle)
        }
        .font(.caption)
        .lineLimit(1)
        .padding(.horizontal)
        .padding(.vertical, 6)
        .background(.bar)
    }
}

private func kilos(_ grams: Int32) -> String {
    (Double(grams) / 1000).formatted(.number.precision(.fractionLength(0...2)))
}
