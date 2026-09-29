import type { ClientInit } from "@sveltejs/kit";

export const init: ClientInit = () => {
  const standalone =
    matchMedia("(display-mode: standalone)").matches ||
    (navigator as Navigator & { standalone?: boolean }).standalone === true;
  const [navigation] = performance.getEntriesByType(
    "navigation",
  ) as PerformanceNavigationTiming[];
  if (
    standalone &&
    navigation?.type !== "reload" &&
    location.pathname.startsWith("/artwork/")
  ) {
    // Redirect Safari's restored page before initializing the editor.
    location.replace("/gallery");
    return new Promise<never>(() => {});
  }
};
