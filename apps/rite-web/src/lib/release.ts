import { ref, type Ref } from "vue";

/**
 * The release tag the site shows.
 *
 * Starts at the version this site was built from (injected from the workspace
 * Cargo.toml) and upgrades to the real latest tag once GitHub answers. The
 * unauthenticated API allows 60 requests per hour per IP, so a rate-limited
 * visitor keeps the build-time value rather than seeing nothing — but
 * `resolved` stays false, and the UI says "packaged with this site" instead of
 * claiming it is the latest release.
 */
const RELEASES_API = "https://api.github.com/repos/undercurrent-labs/rite/releases/latest";

const tag = ref(__RITE_VERSION__);
const resolved = ref(false);
let started = false;

async function fetchLatest(): Promise<void> {
  try {
    const res = await fetch(RELEASES_API, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!res.ok) return;
    const data = (await res.json()) as { tag_name?: string };
    if (data.tag_name) {
      tag.value = data.tag_name;
      resolved.value = true;
    }
  } catch {
    /* keep the build-time value */
  }
}

/** Shared across views so a page visit costs at most one API call. */
export function useLatestTag(): { tag: Ref<string>; resolved: Ref<boolean> } {
  if (!started) {
    started = true;
    void fetchLatest();
  }
  return { tag, resolved };
}
