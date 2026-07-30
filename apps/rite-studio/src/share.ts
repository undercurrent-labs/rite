/** Stateless share links: the editor state as base64url JSON in the URL fragment. */

export type ShareState = {
  source?: string;
  dialect?: string;
  example?: string;
};

/** btoa/atob are byte-oriented, so round-trip UTF-8 explicitly rather than via escape(). */
function toBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(text: string): Uint8Array {
  const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/"));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function encodeShare(state: ShareState): string {
  return toBase64Url(new TextEncoder().encode(JSON.stringify(state)));
}

export function decodeShare(frag: string): ShareState {
  return JSON.parse(new TextDecoder().decode(fromBase64Url(frag))) as ShareState;
}
