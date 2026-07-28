/** Stateless share links via URL fragment (compressed JSON). */

export type ShareState = {
  source?: string;
  dialect?: string;
  example?: string;
};

export function encodeShare(state: ShareState): string {
  const json = JSON.stringify(state);
  // base64url
  if (typeof btoa !== "undefined") {
    return btoa(unescape(encodeURIComponent(json)))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
  }
  return Buffer.from(json, "utf8").toString("base64url");
}

export function decodeShare(frag: string): ShareState {
  const b64 = frag.replace(/-/g, "+").replace(/_/g, "/");
  if (typeof atob !== "undefined") {
    const json = decodeURIComponent(escape(atob(b64)));
    return JSON.parse(json);
  }
  return JSON.parse(Buffer.from(b64, "base64").toString("utf8"));
}
