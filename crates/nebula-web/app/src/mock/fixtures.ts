// Made-up sessions for `pnpm dev`. Nothing here is a real project.
import type { Dialog, Project, Session } from "../api/contract.ts";

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
    prUrl: null,
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
      branch: "acme-900-gps-link-recovery",
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
      branch: "board-dialog-question",
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
    }),
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
