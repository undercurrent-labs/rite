# Hashing and encoding

`@crypto` is the odd capability: almost none of it is an effect.

```rite browser
! @console.println(@crypto.sha256("abc"))
```

No `!` on the `@crypto.sha256` call, and no `--allow` flag anywhere. A digest is a
function of its argument — it observes nothing outside the program and returns the
same answer on every machine forever — so it is marked and permissioned exactly like
`str()` or `upper()`: not at all. Compare [Effects and capabilities](effects.md),
where the rule is that a call is effectful when it observes *or* changes outside
state. Hashing does neither.

The exception is `@crypto.random_bytes`, which reads the machine's entropy pool.
That one is marked, and it rides the `random` permission rather than a new one.

## The surface

| Function | Answers | Marked? |
|---|---|---|
| `@crypto.sha256(s)` | hex digest | no |
| `@crypto.sha512(s)` | hex digest | no |
| `@crypto.hmac_sha256(key, message)` | hex digest | no |
| `@crypto.constant_time_eq(a, b)` | `true` / `false` | no |
| `@crypto.base64_encode(s)` | string | no |
| `@crypto.base64_decode(s)` | **result** | no |
| `@crypto.hex_encode(s)` | string | no |
| `@crypto.hex_decode(s)` | **result** | no |
| `@crypto.random_bytes(n)` | hex string, `2n` characters | **yes** — needs `random` |

Strings in, strings out. A digest is lowercase hex rather than raw bytes because Rite
strings are text, and hex is the form you were going to print, log or compare anyway.

## Digests

```rite browser
! @console.println(@crypto.sha256("abc"))
! @console.println(@crypto.sha512(""))
```

```text
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e
```

Those are the published FIPS 180-4 vectors, and the same two strings appear in the
test suite for exactly that reason: a hash that only agrees with itself is what a
*wrong* hash looks like.

Digests run over the UTF-8 bytes of the string, not over characters, so
`@crypto.sha256("é")` hashes two bytes.

## HMAC

`hmac_sha256(key, message)` is the keyed version — the thing to reach for when you
are signing a webhook payload or a session cookie, and the thing to reach for
*instead of* `sha256(secret + message)`:

```rite browser
signature ← @crypto.hmac_sha256("Jefe", "what do ya want for nothing?")
! @console.println(signature)
```

```text
5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
```

(RFC 4231's second test vector.) The argument order is key first, message second,
and swapping them silently produces a different, wrong signature — there is no type
that can catch that for you.

## Comparing digests

Reach for `constant_time_eq` rather than `=` whenever one side is a secret:

```rite browser
expected ← @crypto.hmac_sha256("k3y", "payload")
supplied ← @crypto.hmac_sha256("k3y", "payload")
? @crypto.constant_time_eq(expected, supplied) ⟦
  ! @console.println("signature ok")
⟧ : ⟦
  ! @console.println("rejected")
⟧
```

```text
signature ok
```

`=` stops at the first differing byte, and how long it took to say "no" tells an
attacker how much of the prefix they guessed right. `constant_time_eq` always walks
the whole string. It does *not* hide the length — for digests and tokens the length
is fixed and public anyway.

## base64 and hex

```rite browser
! @console.println(@crypto.base64_encode("foobar"))
! @console.println(@crypto.hex_encode("foobar"))
```

```text
Zm9vYmFy
666f6f626172
```

The decoders answer a [result](results.md), because their input is normally
untrusted — a header, a query parameter, a file someone else wrote:

```rite browser
message ← ~ @crypto.base64_decode("Zm9vYmFy") ⟦
  ok text → text
  err e → "rejected: {e.message}"
⟧
! @console.println(message)

message2 ← ~ @crypto.base64_decode("Zg=") ⟦
  ok text → text
  err e → "rejected: {e.message}"
⟧
! @console.println(message2)
```

```text
foobar
rejected: length 3 is not a multiple of 4 (input must be padded)
```

The error payload is a record shaped like `@json.decode`'s — `kind` and `message` —
so one handler reads both.

`base64_decode` is strict RFC 4648: padded, canonical, standard alphabet. It rejects
whitespace, the URL-safe `-_` alphabet, unpadded input, and non-canonical trailing
bits (`"Zh=="` is refused even though a lenient decoder would call it `"f"`). Two
systems that disagree about which strings are valid base64 is how a signature check
gets walked around; a decoder that guesses is worse than one that says no.

Decoding can also produce bytes that are not text. `@crypto.hex_decode("ff")` is a
perfectly good hex string whose byte is not valid UTF-8, so it answers `err` rather
than quietly substituting a replacement character into a value you were about to
compare.

## Random bytes

```rite browser
token ← ! @crypto.random_bytes(16)
! @console.println(len(token))
```

```text
32
```

Sixteen bytes, thirty-two hex characters. This is the one call in the capability that
takes a `!`, and the one that consults a permission — `random`, which is allowed by
default, so `--deny random` switches it off along with `@random`.

**It ignores `@random.seed`.** `@random` is a seedable, reproducible generator, which
is why the book tells you to seed it; `@crypto.random_bytes` draws from the operating
system's cryptographic generator instead, so a script that pins a seed for
reproducible dice rolls does not thereby pin its session tokens. That asymmetry is
deliberate. If you want the value to be the same on the next run, you want `@random`
and you should not be minting a secret with it.

## What is deliberately not here

There is no `@crypto.encrypt`, no AES, no RSA, no raw block cipher, and no argument
anywhere that asks you to pick an IV or a mode.

That is a decision, not a gap. An `encrypt(key, iv, mode, data)` on the host surface
is an invitation to ship ECB, or to reuse a nonce, and neither mistake announces
itself — the ciphertext looks like ciphertext either way. Authenticated encryption
only has one safe shape (one construction, a nonce the caller never sees, and a
failure that is a `Result`), and that is a package's job: a future `cipher` package
can expose `seal` / `open` and nothing else, and can be versioned when the
construction is replaced. The host capability is the wrong place to put a decision
that has to be revisited.

Password hashing (argon2, bcrypt, scrypt) is deferred for a related reason rather
than rejected. It needs cost parameters, a stored-format contract, and a migration
story for rehashing when the cost changes — that is a design with state in someone's
database, not a function of its arguments, and dropping `bcrypt(s)` into a table of
pure transforms would misrepresent it.

What is here is the part that is safe to expose as plain functions: fingerprinting,
signing and verifying, and the two encodings those always travel with.

## Browser and Studio

`@crypto` is built to be browser-safe: string in, string out, no filesystem, no
socket, no subprocess. Nothing in it needs the native host the way `@fs` or
`@process` do.

Hosted [Studio](/studio) cannot reach it yet all the same: its WASM bundle registers
no capability host, so `@crypto` is in the same position there as `@json` and `@csv`
— fine under the CLI and under local `rite studio`, unavailable in the browser build.
That is a packaging gap in `rite-wasm` rather than anything about this capability;
see [Browser & Studio](browser.md).

## Next

[Databases](db.md) — `@db`, SQL, and transactions.
