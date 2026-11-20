// Made-up sessions, issues and a day for `pnpm dev`. Nothing here is a real project.
import type { Dialog, Issue, Project, Session, Today, Turn } from "../api/contract.ts";

export interface MockSession extends Session {
  reply: string | null;
  replyAt: number | null;
  answers: string[];
  dialog: Dialog | null;
}

const MIN = 60_000;

export const projects: Project[] = [
  { id: "p-nebula", name: "nebula", space: null, team: null },
  { id: "p-rover", name: "rover-firmware", space: "Acme", team: "ACME" },
  { id: "p-shop", name: "shopfront", space: "Acme", team: "ACME" },
  { id: "p-docs", name: "docs-site", space: null, team: "DOC" },
  { id: "p-ledger", name: "ledger", space: null, team: "ACME" },
];

const projectName = (id: string) => projects.find((p) => p.id === id)!.name;

export function session(
  now: number,
  s: Partial<MockSession> & Pick<Session, "id" | "name" | "projectId" | "status"> & { ago: number },
): MockSession {
  const { ago, ...rest } = s;
  return {
    projectName: projectName(s.projectId),
    branch: "main",
    rootCheckout: false,
    harness: "claude",
    role: "worker",
    statusChangedAt: now - ago * MIN,
    unseen: false,
    why: null,
    reportWord: null,
    issueUrl: null,
    issueKey: null,
    issue: null,
    prUrl: null,
    prState: null,
    reply: null,
    replyAt: null,
    answers: [],
    dialog: null,
    ...rest,
  };
}

export function sessions(now: number): MockSession[] {
  return [
    session(now, {
      id: "orch",
      name: "orchestrator",
      projectId: "p-nebula",
      role: "orchestrator",
      status: "finished",
      ago: 38,
      rootCheckout: true,
      reply: "I ranked the board: the rover captain's question first, then the board PR, then the dialog in nebula.",
      replyAt: now - 38 * MIN,
    }),
    session(now, {
      id: "rover-cap",
      name: "GPS link recovery",
      projectId: "p-rover",
      role: "captain",
      status: "finished",
      unseen: true,
      ago: 12,
      reportWord: "QUESTION",
      why: "QUESTION: keep the udev rule, or retry in code?",
      branch: "main",
      rootCheckout: true,
      issueUrl: "https://linear.app/acme/issue/ACME-900/gps-link-recovery",
      issueKey: "ACME-900",
      replyAt: now - 12 * MIN,
      reply: [
        "Both workers reproduce the drop after a USB reset of the GPS receiver.",
        "",
        "- `gps-udev` re-binds the device with a udev rule. Six lines and no code change, but it only covers this reset.",
        "- `gps-retry` retries the open with backoff in the driver. More code, and it covers every reset.",
        "",
        "| | udev rule | retry in code |",
        "|---|---|---|",
        "| Lines changed | 6 | 140 |",
        "| Covers | USB reset | every reset |",
        "| Tests | manual | `cargo test -p gps` |",
        "",
        "QUESTION: keep the udev rule, or retry in code?",
        "NEXT: udev rule | retry in code | show both diffs",
      ].join("\n"),
      answers: ["udev rule", "retry in code", "show both diffs"],
    }),
    session(now, {
      id: "rover-udev",
      name: "gps-udev",
      projectId: "p-rover",
      status: "finished",
      ago: 8,
      branch: "gps-udev",
      prUrl: "https://github.com/acme/rover-firmware/pull/31",
      prState: "open",
      why: "A udev rule re-binds the receiver after a USB reset",
      reply: "DONE: a udev rule re-binds the receiver after a USB reset",
      replyAt: now - 8 * MIN,
    }),
    session(now, {
      id: "rover-retry",
      name: "gps-retry",
      projectId: "p-rover",
      status: "running",
      ago: 6,
      branch: "gps-retry",
    }),
    session(now, {
      id: "neb-dialog",
      name: "Board reads a dialog's question",
      projectId: "p-nebula",
      status: "needsFeedback",
      ago: 4,
      branch: "acme-951-board-dialog-question",
      why: "Bash: cargo build --release",
      reply: "The board now reads what a permission dialog is about. Building to check the release profile.",
      replyAt: now - 5 * MIN,
      dialog: {
        question: "Do you want to proceed?",
        options: [
          { number: 1, label: "Yes" },
          { number: 2, label: "Yes, and don't ask again for cargo build commands in this project" },
          { number: 3, label: "No, and tell Claude what to do differently" },
        ],
      },
    }),
    session(now, {
      id: "shop-retries",
      name: "Checkout retries",
      projectId: "p-shop",
      status: "finished",
      ago: 2 * 24 * 60,
      reportWord: "BLOCKED",
      why: "BLOCKED: the staging API key expired; add a new one to .env.staging",
      branch: "checkout-retries",
      issueUrl: "https://github.com/acme/shopfront/issues/88",
      issueKey: "#88",
      prUrl: "https://github.com/acme/shopfront/pull/212",
      prState: "draft",
      reply: [
        "The retry wrapper is in and the unit tests pass.",
        "",
        "```ts",
        "await withRetry(() => charge(order), { tries: 3, backoffMs: 200 });",
        "```",
        "",
        "The end-to-end run against staging fails at login: the API key in `.env.staging` expired on Friday.",
        "",
        "> The log said <b>401</b>, and raw HTML in a reply <script>alert('stays text')</script> is shown as typed.",
        "",
        "BLOCKED: the staging API key expired; add a new one to `.env.staging`",
        "NEXT: I added it, run again | skip the staging run",
      ].join("\n"),
      replyAt: now - 2 * 24 * 60 * MIN,
      answers: ["I added it, run again", "skip the staging run"],
    }),
    session(now, {
      id: "neb-f8",
      name: "F8 hands the keys to the board",
      projectId: "p-nebula",
      status: "finished",
      unseen: true,
      ago: 2,
      branch: "f8-board-keys",
      prUrl: "https://github.com/acme/nebula/pull/57",
      prState: "open",
      why: "F8 hands the keys to the board, and Esc gives them back",
      reply: [
        "F8 now moves the keys to the board and Esc hands them back to the session.",
        "",
        "1. `j`/`k` walk the rows, Enter shows one.",
        "2. The lit row keeps its fill while the keys are away.",
        "",
        "See [the PR](https://github.com/acme/nebula/pull/57) for the input-parity tests.",
        "",
        "DONE: F8 hands the keys to the board, and Esc gives them back",
        "NEXT: merge it | add a TUI shot",
      ].join("\n"),
      replyAt: now - 2 * MIN,
      answers: ["merge it", "add a TUI shot"],
    }),
    session(now, {
      id: "shop-cart",
      name: "Persist the cart across devices",
      projectId: "p-shop",
      status: "finished",
      unseen: true,
      ago: 7,
      branch: "acme-940-persist-cart",
      prUrl: "https://github.com/acme/shopfront/pull/219",
      prState: "merged",
      why: "The cart is stored per account and merged on sign-in",
      reply: [
        "The cart now lives on the account, so a phone and a laptop see the same one.",
        "",
        "A guest cart is merged into the account's on sign-in, newest line first.",
        "",
        "DONE: the cart is stored per account and merged on sign-in",
      ].join("\n"),
      replyAt: now - 7 * MIN,
    }),
    session(now, {
      id: "docs-install",
      name: "Rewrite the install guide",
      projectId: "p-docs",
      status: "running",
      ago: 25,
      branch: "install-guide",
    }),
    session(now, {
      id: "neb-web",
      name: "Web app for nebula serve",
      projectId: "p-nebula",
      status: "running",
      ago: 3,
      branch: "web-ui",
      issueUrl: "https://linear.app/acme/issue/ACME-936/web-app",
      issueKey: "ACME-936",
    }),
    session(now, {
      id: "ledger-cap",
      name: "captain",
      projectId: "p-ledger",
      role: "captain",
      status: "finished",
      ago: 50,
      rootCheckout: true,
      reply: "Handed the bank import to a worker. I'll report when it lands.\n\nDONE: handed the bank import to a worker",
      replyAt: now - 50 * MIN,
    }),
    session(now, {
      id: "ledger-import",
      name: "Import bank statements",
      projectId: "p-ledger",
      status: "running",
      ago: 12,
      branch: "bank-import",
    }),
    session(now, { id: "docs-fresh", name: "docs-site", projectId: "p-docs", status: "fresh", ago: 90, rootCheckout: true }),
    session(now, { id: "shop-fresh", name: "shopfront", projectId: "p-shop", status: "fresh", ago: 300, rootCheckout: true }),
    session(now, {
      id: "neb-e2e",
      name: "Flaky e2e repro",
      projectId: "p-nebula",
      status: "terminated",
      ago: 140,
      branch: "e2e-flake",
      prUrl: "https://github.com/acme/nebula/pull/49",
      prState: "closed",
    }),
  ];
}

const you = (text: string, at: number): Turn => ({ role: "user", text, at });
const claude = (text: string, at: number): Turn => ({ role: "assistant", text, at });

/// The turns before each session's current reply, oldest first: a session's
/// thread is these, then its reply. A session with none here is its reply
/// alone, and `neb-web`, at work on its last prompt, has none yet.
export function histories(now: number): Record<string, Turn[]> {
  const ago = (minutes: number) => now - minutes * MIN;
  return {
    "rover-cap": [
      you(
        "ACME-900: the rover loses its GPS fix after a USB reset of the receiver and never gets it back. Find out why, and try two fixes side by side.",
        ago(58),
      ),
      claude(
        [
          "Reproduced it on the bench: after `usbreset` the receiver comes back as `/dev/ttyACM1`, and the driver keeps reading `/dev/ttyACM0`.",
          "",
          "I started two workers:",
          "",
          "- `gps-udev` pins the device name with a udev rule.",
          "- `gps-retry` reopens the port with backoff when reads fail.",
          "",
          "I'll compare them once both report.",
          "",
          "DONE started two workers on ACME-900",
        ].join("\n"),
        ago(52),
      ),
      you("Keep the retry under 2 s in total: the nav loop times out after 3.", ago(40)),
      claude(
        "Told `gps-retry` to cap its backoff at 1.8 s in all: 100, 200, 400 and 800 ms, then give up and log it.\n\nDONE briefed gps-retry on the 2 s cap",
        ago(38),
      ),
      you("How are the two doing?", ago(14)),
    ],
    "neb-dialog": [
      you(
        "ACME-951: the board says needs you for a permission dialog, but not what it asks. Read the dialog's question off the screen and show it under the row.",
        ago(30),
      ),
      claude(
        [
          "The question sits two rows above the options, so `dialog::choice_in` can return it with them:",
          "",
          "```rust",
          "pub struct Choice {",
          "    pub question: String,",
          "    pub options: Vec<(u32, String)>,",
          "}",
          "```",
          "",
          "I'll wire it into the row's line next.",
        ].join("\n"),
        ago(24),
      ),
      you("For a Bash call, name the command too, like `Bash: cargo test`.", ago(12)),
    ],
    "shop-retries": [
      you(
        "#88: checkout fails when the payment API times out once, as in this trace:\n[Image #1]\n\nWrap the charge call in a retry with backoff, three tries at most.",
        ago(2 * 24 * 60 + 50),
      ),
      claude(
        [
          "Found it in `src/checkout/charge.ts`: a timeout throws `GatewayTimeout`, and nothing catches it.",
          "",
          "1. A `withRetry` helper with jittered backoff.",
          "2. Retry only a `GatewayTimeout` or a `503`, never a declined card.",
          "3. A unit test for each.",
          "",
          "QUESTION: go ahead with this plan?",
          "NEXT: go ahead | retry declines too",
        ].join("\n"),
        ago(2 * 24 * 60 + 40),
      ),
      you("go ahead", ago(2 * 24 * 60 + 35)),
    ],
    "neb-f8": [
      you("F8 should move the keys to the board, so `j` and `k` walk its rows without touching the session. Esc hands them back.", ago(40)),
      claude(
        [
          "Done in `event_loop/keys.rs`: F8 sets the focus on the board, and the session gets no keys while it has it.",
          "",
          "One thing to decide: the lit row loses its fill while the keys are away, so it looks as if nothing is selected.",
          "",
          "QUESTION: keep the lit row's fill while the board has the keys?",
          "NEXT: keep it | dim it",
        ].join("\n"),
        ago(30),
      ),
      you("keep it", ago(28)),
      claude("Kept, and added input-parity tests for F8, Esc and a click on the board.\n\nDONE the lit row keeps its fill", ago(20)),
      you("Open a PR for it.", ago(6)),
    ],
    "neb-web": [
      you("ACME-936: a web app for `nebula serve`: the board, each session's last reply, and a box for a follow-up. Plain CSS, no framework.", ago(3 * 60)),
      claude(
        [
          "The server side is in: `nebula serve` subscribes to the daemon as the TUI does, and serves:",
          "",
          "| Endpoint | What |",
          "|---|---|",
          "| `GET /api/state` | the whole board |",
          "| `GET /api/events` | a state on every change |",
          "| `GET /api/sessions/:id` | one session's reply |",
          "",
          "The React app is next.",
          "",
          "DONE served the board as JSON",
        ].join("\n"),
        ago(2 * 60),
      ),
      you("Now the app: the list on the left, the session on the right, in Catppuccin Macchiato.", ago(3)),
    ],
  };
}

/// The sessions whose transcripts hold turns older than their thread shows.
export const olderTurns = new Set(["rover-cap"]);

const issue = (
  now: number,
  key: string,
  title: string,
  over: Partial<Issue> & { ago: number },
): Issue => {
  const { ago, ...rest } = over;
  const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return {
    key,
    url: `https://linear.app/acme/issue/${key}/${slug}`,
    title,
    priority: 0,
    state: "Todo",
    stateType: "unstarted",
    assignee: "Ada Example",
    branchName: `ada/${key.toLowerCase()}-${slug}`,
    updatedAt: now - ago * MIN,
    team: key.split("-")[0]!,
    project: null,
    started: null,
    lastProject: null,
    ...rest,
  };
};

const started = { state: "In Progress", stateType: "started" } as const;

/// The open issues assigned to you: some a session works on, the rest in
/// the lanes, and one in the backlog, which neither lane shows. ACME's
/// issues start in rover-firmware, shopfront or ledger, and DOC's only in
/// docs-site.
export function issues(now: number): Issue[] {
  return [
    issue(now, "ACME-900", "GPS link recovery after a USB reset", { ...started, priority: 2, ago: 15, project: "Rover v2" }),
    issue(now, "ACME-936", "Web app for nebula serve", { ...started, priority: 3, ago: 40, project: "Tooling" }),
    issue(now, "ACME-951", "Board reads a dialog's question", { ...started, priority: 1, ago: 5, project: "Tooling" }),
    issue(now, "ACME-940", "Persist the cart across devices", { state: "In Review", stateType: "started", priority: 3, ago: 8, project: "Checkout" }),
    issue(now, "ACME-912", "Retry the OTA upload after a dropped link", { ...started, priority: 1, ago: 3 * 60, project: "Rover v2" }),
    issue(now, "ACME-944", "Sign the release builds of the shop", { ...started, priority: 3, ago: 26 * 60, project: "Checkout" }),
    issue(now, "ACME-930", "Cache the docs search index between deploys", { state: "In Review", stateType: "started", ago: 50 }),
    issue(now, "ACME-958", "Return to the dock below 15% battery", { priority: 2, ago: 2 * 60, project: "Rover v2" }),
    issue(now, "ACME-960", "Show the refund reason on the receipt", { priority: 3, ago: 30, project: "Checkout" }),
    issue(now, "ACME-947", "Rename the ledger's CSV export columns", { priority: 4, ago: 4 * 24 * 60 }),
    issue(now, "ACME-962", "Spike: a queue in place of the nightly cron", { ago: 6 * 24 * 60 }),
    issue(now, "DOC-31", "Fix the broken anchors in the API reference", { priority: 3, ago: 3 * 24 * 60 }),
    issue(now, "ACME-970", "Dark mode for the receipts page", { state: "Backlog", stateType: "backlog", priority: 4, ago: 9 * 24 * 60 }),
  ];
}

/// The orchestrator's picks by `NEBULA_MOCK_FOCUS`, each a candidate's
/// key: the issue joined to a session, an issue in a lane, or a session.
/// `fewer` picks one issue and leaves every row that needs you out, `none`
/// picks nothing, and `gone` picks what is no candidate any more.
export function picks(scene: string): { key: string; reason: string }[] {
  const all = [
    {
      key: "ACME-900",
      reason: "The captain asks: udev rule or retry in code? A worker waits on the answer, so this unblocks the most work.",
    },
    {
      key: "ACME-912",
      reason: "Urgent, and rovers in the field drop OTA uploads: nobody works on it yet.",
    },
    {
      key: "ACME-951",
      reason: "Waits on a permission to run cargo build. One key gets it going again.",
    },
  ];
  switch (scene) {
    case "fewer":
      return [all[1]!];
    case "none":
      return [];
    case "gone":
      return [
        { key: "ACME-933", reason: "The release notes wait on your review." },
        { key: "ACME-927", reason: "The flaky login test fails one run in five." },
      ];
    default:
      return all;
  }
}

/// What a rank in the mock picks next: the same three, the board PR first.
export const reranked = [
  { key: "neb-f8", reason: "Finished two minutes ago with PR #57 open. Merging it lets the board work move on." },
  { key: "ACME-900", reason: "The captain still waits on udev rule or retry in code, and a worker waits on it." },
  { key: "ACME-912", reason: "Urgent, and nobody works on it yet." },
];

/// The day a toolbox writes to `today_file`: a few todos late, a lift
/// today, two projects gone quiet and a nightly run that went well, by
/// `NEBULA_MOCK_TODAY`: `trouble` has the todos and the training unread,
/// the nightly run failed and the kill switch on.
export function today(now: number, trouble: boolean): Today {
  const date = (days: number) => {
    const d = new Date(now + days * 86_400_000);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };
  const todo = (title: string, days: number, id: number) => ({ title, due: date(days), url: `https://todo.example.com/task/${id}` });
  const nightAt = new Date(now);
  nightAt.setHours(3, 17, 0, 0);
  return {
    at: nightAt.getTime() + 4_000,
    todos: trouble
      ? { count: 0, overdue: 0, top: [], url: "https://todo.example.com/upcoming", problem: "todo: the todo list refused GET /tasks" }
      : {
          count: 7,
          overdue: 5,
          top: [
            todo("Renew the car insurance before the old policy lapses", -12, 1),
            todo("Book the dentist", -9, 2),
            todo("Return the library books", -2, 3),
          ],
          url: "https://todo.example.com/upcoming",
          problem: null,
        },
    training: trouble
      ? { block: null, today: null, url: null, lifts: 0, liftsPerWeek: 4, problem: "the lifting log refused GET /workout/next: 401 Invalid API key" }
      : {
          block: "Week 3, build",
          today: "Day 2 Lower, squat and hinge",
          url: "obsidian://open?path=%2Fnotes%2FTraining%2FNext%20session.md",
          lifts: 1,
          liftsPerWeek: 4,
          problem: null,
        },
    quiet: [
      { name: "Garden shed", brain: "home", days: 47, url: "obsidian://open?path=%2Fnotes%2FGarden%20shed.md" },
      { name: "Shopfront checkout redesign", brain: "acme", days: 41, url: "obsidian://open?path=%2Fnotes%2FCheckout.md" },
      { name: "Bike rebuild", brain: "home", days: 31, url: null },
    ],
    nightly: trouble
      ? { at: nightAt.getTime() - 86_400_000 * 2, ok: false, why: "home: pulling conflicts with the remote; the local commits are kept and not pushed" }
      : { at: nightAt.getTime(), ok: true, why: null },
    off: trouble,
  };
}
