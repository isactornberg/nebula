// Made-up sessions and issues for `pnpm dev`. Nothing here is a real project.
import type { Dialog, Issue, Project, Session } from "../api/contract.ts";

export interface MockSession extends Session {
  reply: string | null;
  replyAt: number | null;
  answers: string[];
  dialog: Dialog | null;
}

const MIN = 60_000;

export const projects: Project[] = [
  { id: "p-nebula", name: "nebula", space: null },
  { id: "p-rover", name: "rover-firmware", space: "Acme" },
  { id: "p-shop", name: "shopfront", space: "Acme" },
  { id: "p-docs", name: "docs-site", space: null },
  { id: "p-ledger", name: "ledger", space: null },
];

const projectName = (id: string) => projects.find((p) => p.id === id)!.name;

function session(
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
    team: "ACME",
    project: null,
    ...rest,
  };
};

const started = { state: "In Progress", stateType: "started" } as const;

/// The open issues assigned to you: some a session works on, the rest in
/// the lanes, and one in the backlog, which neither lane shows.
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
    issue(now, "ACME-970", "Dark mode for the receipts page", { state: "Backlog", stateType: "backlog", priority: 4, ago: 9 * 24 * 60 }),
  ];
}

/// The orchestrator's picks, until a session they name goes away.
export const picks = [
  {
    sessionId: "rover-cap",
    reason: "The captain asks: udev rule or retry in code? A worker waits on the answer, so this unblocks the most work.",
  },
  {
    sessionId: "neb-f8",
    reason: "Finished two minutes ago with PR #57 open. Merging it lets the board work move on.",
  },
  {
    sessionId: "neb-dialog",
    reason: "Waits on a permission to run cargo build. One key gets it going again.",
  },
];
