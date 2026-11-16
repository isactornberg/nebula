// nebula's service worker, registered on a phone paired to the Mac alone:
// it shows a notification the daemon pushes when a session needs you, and
// opens the page on that session when it is tapped. Nothing else: no
// caching, no offline.

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("push", (event) => {
  let notice = {};
  try {
    notice = event.data ? event.data.json() : {};
  } catch {}
  event.waitUntil(
    self.registration.showNotification(notice.title || "nebula", {
      body: notice.body || "A session needs you.",
      tag: notice.tag,
      renotify: !!notice.tag,
      icon: "/icon-192.png",
      data: { url: notice.url || "/" },
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const path = new URL(event.notification.data?.url || "/", self.location.origin).pathname;
  event.waitUntil(
    (async () => {
      const open = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      const page = open.find((c) => c.focused) ?? open.find((c) => c.visibilityState === "visible") ?? open[0];
      if (!page) return self.clients.openWindow(path);
      await page.focus();
      page.postMessage({ open: path });
    })(),
  );
});
