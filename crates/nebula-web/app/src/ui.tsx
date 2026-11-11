import type { AnchorHTMLAttributes, MouseEvent } from "react";
import { routePath, type Flash, type Route, type Tone } from "./model";

/// A real link to `to`: a plain click calls `onGo` in place of loading the
/// page, a modified one (new tab, new window) is the browser's.
export function RouteLink({
  to,
  onGo,
  ...rest
}: { to: Route; onGo: () => void } & AnchorHTMLAttributes<HTMLAnchorElement>) {
  const click = (e: MouseEvent) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    onGo();
  };
  return <a {...rest} href={routePath(to)} onClick={click} />;
}

export function Dot({ tone }: { tone: Tone }) {
  return <span aria-hidden="true" className={`dot tone-${tone}`} />;
}

export function External(props: AnchorHTMLAttributes<HTMLAnchorElement>) {
  return <a {...props} target="_blank" rel="noreferrer" />;
}

/// A word said for a moment under the control it is about. The live
/// region stays in the page, so a screen reader hears each word as it
/// comes; `seq` tells two of the same words apart.
export function FlashSpot({ flash }: { flash: (Flash & { seq: number }) | null }) {
  return (
    <span className="flash" role="status">
      {flash && (
        <span key={flash.seq} className={flash.error ? "flash-text error" : "flash-text"}>
          {flash.text}
        </span>
      )}
    </span>
  );
}
