import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { canNotify, keyBytes, NotifyView, type Browser, type NotifyState } from "./Notify";

const browser = (over: Partial<Browser> = {}): Browser => ({
  push: true,
  ios: true,
  standalone: true,
  permission: "default",
  ...over,
});

const view = (state: NotifyState, over: Partial<Parameters<typeof NotifyView>[0]> = {}) =>
  renderToStaticMarkup(
    <NotifyView state={state} busy={false} error={null} ios onOn={() => {}} onOff={() => {}} onClose={() => {}} {...over} />,
  );

describe("Notify", () => {
  it("asks to install the page first in Safari on iOS, where only the Home Screen app is notified", () => {
    expect(canNotify(browser({ standalone: false, push: false }))).toBe("install");
    expect(canNotify(browser())).toBe("ask");
    expect(canNotify(browser({ ios: false, standalone: false }))).toBe("ask");
    expect(canNotify(browser({ ios: false, push: false }))).toBe("unsupported");
    expect(canNotify(browser({ permission: "denied" }))).toBe("denied");
    expect(canNotify(browser({ permission: "granted" }))).toBe("ask");
  });

  it("reads the server's key as the 65 bytes of a P-256 point", () => {
    const key = keyBytes("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8");
    expect(key.length).toBe(65);
    expect(key[0]).toBe(4);
    expect(key[64]).toBe(0x0f);
  });

  it("offers to turn notifications on while they are off, and off while they are on", () => {
    const off = view("off");
    expect(off).toContain("Off for this phone.");
    expect(off).toContain('<button type="button" class="btn primary">Turn on notifications</button>');
    const on = view("on");
    expect(on).toContain('<span aria-hidden="true" class="sdot green"></span>On for this phone.');
    expect(on).toContain(">Turn off</button>");
    expect(on).not.toContain("Turn on notifications");
    expect(view("off", { busy: true })).toContain('disabled="">Turn on notifications');
  });

  it("says how to allow notifications once they are blocked", () => {
    const ios = view("denied");
    expect(ios).toContain("Notifications are blocked for nebula on this phone.");
    expect(ios).toContain("open Settings, then Notifications, then nebula, and turn on Allow Notifications");
    expect(ios).not.toContain("<button type=\"button\" class=\"btn primary\"");
    expect(view("denied", { ios: false })).toContain("site settings. Then come back here.");
  });

  it("says to add the page to the Home Screen first in Safari", () => {
    const html = view("install");
    expect(html).toContain("iOS shows notifications only from the Home Screen app.");
    expect(html).toContain("tap Share, then Add to Home Screen, and turn them on in the nebula it adds.");
    expect(html).not.toContain("Turn on notifications");
    expect(view("unsupported")).toContain("This browser cannot show notifications from a web page.");
  });

  it("says what went wrong", () => {
    expect(view("off", { error: "This browser did not turn them on: no." })).toContain(
      '<p class="error" role="alert">This browser did not turn them on: no.</p>',
    );
  });
});
