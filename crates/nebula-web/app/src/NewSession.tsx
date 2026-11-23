import { useEffect, useState } from "react";
import { api, type Read } from "./api/client";
import type { NewSession, Project, SessionStarted } from "./api/contract";
import { ComposeForm, useCompose } from "./Compose";
import { BackLink, type Layout } from "./Detail";
import { parseRoute, type Route } from "./model";
import { Icon, RouteLink } from "./ui";
import "./new.css";

const PROJECT_KEY = "nebula.newProject";

type Store = Pick<Storage, "getItem" | "setItem">;

function store(): Store | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

/// The project New picks first: the one the list is filtered to, else the
/// one this device started a session in last, else the first.
export function firstProject(projects: Project[], filter: string | null, kept: Store | undefined): string | null {
  const known = (id: string | null) => id !== null && projects.some((p) => p.id === id);
  if (known(filter)) return filter;
  let last: string | null = null;
  try {
    last = kept?.getItem(PROJECT_KEY) ?? null;
  } catch {
    // Not kept on this device, then.
  }
  return known(last) ? last : (projects[0]?.id ?? null);
}

export function keepProject(kept: Store | undefined, id: string) {
  try {
    kept?.setItem(PROJECT_KEY, id);
  } catch {
    // Not remembered across reloads, then.
  }
}

export function newBody(projectId: string, text: string, images: string[]): NewSession {
  return images.length ? { projectId, text, images } : { projectId, text };
}

/// Asks the server to start the session, and once it stands remembers its
/// project as this device's last.
export async function startNew(
  projectId: string,
  text: string,
  images: string[],
  kept: Store | undefined,
  post: (body: NewSession) => Promise<Read<SessionStarted>> = api.newSession,
): Promise<Read<SessionStarted>> {
  const got = await post(newBody(projectId, text, images));
  if (got.ok) keepProject(kept, projectId);
  return got;
}

/// What the new session's thread says of its start, when its prompt may
/// not have arrived whole: that it started all the same, and why.
export function startedNote(got: SessionStarted): string | null {
  return got.problem ? `The session started. ${got.problem}` : null;
}

/// Where New's answer goes, at `pathname` once the server answered: while
/// New is still open, the session's thread opens in its place; else the
/// page stays where it went, and says the session started.
export function afterStart(pathname: string, got: SessionStarted): { kind: "open"; to: Route; note: string | null } | { kind: "away" } {
  return parseRoute(pathname).kind === "new"
    ? { kind: "open", to: { kind: "session", id: got.id }, note: startedNote(got) }
    : { kind: "away" };
}

/// What a page says of a session New started once the page had left New.
export function StartedAway({ got, onOpen }: { got: SessionStarted; onOpen: () => void }) {
  return (
    <>
      Your new session started.{got.problem && ` ${got.problem}`}{" "}
      <RouteLink to={{ kind: "session", id: got.id }} onGo={onOpen}>
        Open it
      </RouteLink>
    </>
  );
}

/// What New's box held, kept while the page is open, so leaving New and
/// coming back finds it as it was.
let draft = "";

/// New session: a project, and the prompt Claude starts on, with images
/// and dictation as a reply takes them. It opens the session once the
/// server says it stands; a failure says why and keeps what was typed.
export function NewSessionPage({
  projects,
  filter,
  layout,
  back,
  onBack,
  onStarted,
}: {
  projects: Project[];
  /// The project the list is filtered to, if any.
  filter: string | null;
  layout: Layout;
  back: string;
  onBack: () => void;
  onStarted: (started: SessionStarted) => void;
}) {
  const [picked, setPicked] = useState(() => firstProject(projects, filter, store()));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const project = projects.find((p) => p.id === picked) ?? projects[0] ?? null;
  const phone = layout === "phone";
  const start = async (text: string, images: string[]) => {
    if (!project) return false;
    setBusy(true);
    setError(null);
    const got = await startNew(project.id, text, images, store());
    setBusy(false);
    if (!got.ok) {
      setError(got.error);
      return false;
    }
    draft = "";
    onStarted(got.value);
    return true;
  };
  const c = useCompose(start, project ? null : "No project to start a session in.", busy, draft);
  useEffect(() => {
    draft = c.text;
  }, [c.text]);
  useEffect(() => c.box.current?.focus(), []);

  const pick = projects.length > 0 && (
    <label className="new-project">
      <Icon name="folder" size={15} />
      <span className="sr">Project</span>
      <select value={project?.id ?? ""} disabled={busy} onChange={(e) => setPicked(e.target.value)}>
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={13} />
    </label>
  );

  return (
    <main className="sess new-sess" data-layout={layout}>
      <section className="sess-main" aria-labelledby="new-title">
        <header className="sess-head">
          {phone ? (
            <div className="sess-nav">
              <BackLink back={back} onBack={onBack} />
            </div>
          ) : (
            layout === "single" && <BackLink back={back} onBack={onBack} />
          )}
          <div className="sess-top">
            <h1 id="new-title" className="sess-title">
              New session
            </h1>
          </div>
          <div className="new-line">
            {pick}
            <span className="new-how">Claude Code, in its root checkout, on the model Settings → Agents sets</span>
          </div>
        </header>
        <div className="sess-scroll">
          <div className="sess-col">
            <div className="t-gap" />
            {project ? (
              <>
                <p className="t-rule">New session in {project.name}</p>
                <p className="new-say">
                  Your prompt is its first turn, typed in once Claude is up. Its thread opens here as soon as Claude takes it.
                </p>
              </>
            ) : (
              <p className="t-empty">nebula has no project to start a session in yet: add one in the TUI.</p>
            )}
          </div>
        </div>
        <div className="dock">
          <div className="dock-in">
            <ComposeForm
              c={c}
              id="new-text"
              phone={phone}
              words={{
                label: "What Claude should do",
                placeholder: !project ? "No project yet" : phone ? "What should Claude do?" : `What should Claude do in ${project.name}?`,
                to: `New session${project ? ` in ${project.name}` : ""}`,
                send: "Start",
                dictate: "Dictate the prompt",
              }}
            />
            {error && (
              <p className="error dock-error" role="alert">
                {error}
              </p>
            )}
          </div>
        </div>
      </section>
    </main>
  );
}
