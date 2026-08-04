# Vendored fonts

DM Sans (variable, latin) and IBM Plex Mono (400/500, latin), both under the
SIL Open Font License 1.1.

Vendored rather than linked because this app's Content-Security-Policy is
`default-src 'self'` — the same architectural privacy claim as `connect-src`:
the page fetches nothing from anywhere, fonts included. The Rite and Cant sites
load the same two families from Google Fonts; this one cannot, so it serves
them itself and the three sites still read as one family.

- `dm-sans-latin.woff2` — DM Sans variable weight 400–700
- `ibm-plex-mono-latin-400.woff2`, `ibm-plex-mono-latin-500.woff2`
