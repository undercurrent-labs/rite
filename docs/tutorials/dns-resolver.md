# A DNS resolver over `@udp`

**You will build** a DNS client: it encodes a question into wire-format bytes, sends
one datagram, and reads an IPv4 address back out of the reply.

**You need** a Rite install and a DNS server you can reach — the one in
`/etc/resolv.conf` will do.

<!-- ci: local-only -->

> **This tutorial's script is not run in CI**, unlike most. It needs a real
> nameserver, and a documentation build that depends on the network is a build that
> fails for reasons that have nothing to do with the code. It is run locally with
> `cargo test -p rite-cli --test tutorial_scripts -- --ignored`, and every output
> below came from a real run.

This is the tutorial that byte authoring exists for. DNS is a good first binary
protocol: the request fits in about thirty bytes, and every one of them is
something you can point at.

## The question, byte by byte

A query is a 12-byte header, then the name, then two 16-bit fields.

Rite strings are UTF-8 text, and a packet is not text, so payloads are the **bytes**
type. `bytes` converts between them — hand it a string and you get that string's
UTF-8 bytes:

```rite browser
! @console.println(to_hex(bytes("com")))
```

```text
636f6d
```

`bytes` is the one entry point: a string becomes its UTF-8, and a list of numbers
becomes those numbers as bytes. Both forms appear in the next block.

A name is not sent with dots. Each label is preceded by its **length as one byte**,
and the whole thing ends with a zero byte:

```rite browser
◆ label(part) ⟦ ^ concat(bytes([len(part)]), bytes(part)) ⟧

◆ qname(host) ⟦
  ^ concat(reduce(split(host, "."), { |acc, p| concat(acc, label(p)) }, bytes([])), bytes([0]))
⟧

! @console.println(to_hex(qname("example.com")))
```

```text
076578616d706c6503636f6d00
```

Read that back: `07` then `example`, `03` then `com`, then `00`. It is worth
checking a hex dump against the spec once by eye — it is the fastest way to find the
byte you got wrong.

> **`bytes([len(part)])`, not `bytes(str(len(part)))`.** The length is the *value*
> 3, one byte. Converting the string `"3"` gives `0x33`, the digit — and produces a
> name no server will parse. `bytes` takes a list of numbers precisely so a byte can
> be a number rather than a rendering of one.

## The header

```rite browser
! @console.println(to_hex(from_hex("abcd01000001000000000000")?))
```

```text
abcd01000001000000000000
```

Twelve bytes, six 16-bit fields: a request id (`abcd`, ours to choose), flags
(`0100` — a standard query, recursion desired), then one question and zero of
everything else. Written as a hex literal because that is how the spec prints it,
and matching the spec character for character is the point.

## Sending it

```rite native_only
sock ← ! @udp.bind("0.0.0.0:0")?
! @udp.send_to(sock, "100.100.100.100:53", packet)?
reply ← ! @udp.recv_from(sock, 5000)?
! @udp.close(sock)?
```

Two grants, for two different questions:

```bash
rite run resolve.rite --allow net=0.0.0.0 --allow net=100.100.100.100
```

Binding `0.0.0.0` needs a grant because it exposes a socket beyond loopback; sending
to a host needs a grant for **that host**. Binding tells you nothing about where you
may send, which is why one does not imply the other. Get it wrong and the message
says exactly which half is missing:

```text
permission denied: net permission denied for bind on `0.0.0.0:0`: binding `0.0.0.0`
exposes it beyond loopback (only 127.0.0.0/8, ::1 and localhost are allowed by
default). Re-run with `--allow net=0.0.0.0` …
```

## Reading the answer

The reply repeats your header and question, then appends the answer records. So the
first record starts after the 12-byte header, the name you sent, and the four bytes
of type and class:

```rite
first_rr ← 12 + len(qname(host)) + 4
```

Within a record: two bytes of name, two of type, two of class, four of TTL, two of
length — twelve — and then the data. For an `A` record the data is the four octets
of an IPv4 address:

```rite
◆ ipv4_at(packet, at) ⟦
  ^ str(byte_at(packet, at)) + "." + str(byte_at(packet, at + 1)) + "."
    + str(byte_at(packet, at + 2)) + "." + str(byte_at(packet, at + 3))
⟧
```

`byte_at` answers a number, so arithmetic on the wire is ordinary arithmetic. The
answer count lives in the header, two bytes big-endian, which is `high * 256 + low`:

```rite
answers ← byte_at(data, 6) * 256 + byte_at(data, 7)
```

Run it against a real resolver and you get a real address:

```text
query   abcd01000001000000000000076578616d706c6503636f6d0000010001
answers 2
address 172.66.147.243
```

**That address is not stable.** `example.com` answers with more than one A record
and the order rotates, so the script below prints the *shape* of what it found
rather than the value — which is also the honest thing for a test to assert about
something the internet decides.

## The whole script

```rite
// resolve.rite — ask a DNS server for an A record, over @udp.

◆ label(part) ⟦ ^ concat(bytes([len(part)]), bytes(part)) ⟧

◆ qname(host) ⟦
  ^ concat(reduce(split(host, "."), { |acc, p| concat(acc, label(p)) }, bytes([])), bytes([0]))
⟧

◆ query(host) ⟦
  header ← from_hex("abcd01000001000000000000")?
  question ← concat(qname(host), from_hex("00010001")?)
  ^ concat(header, question)
⟧

◆ ipv4_at(packet, at) ⟦
  ^ str(byte_at(packet, at)) + "." + str(byte_at(packet, at + 1)) + "."
    + str(byte_at(packet, at + 2)) + "." + str(byte_at(packet, at + 3))
⟧

◆! main() ⟦
  host ← "example.com"
  server ← "100.100.100.100:53"

  sock ← ! @udp.bind("0.0.0.0:0")?
  packet ← query(host)
  ! println("query   " + to_hex(packet))

  ! @udp.send_to(sock, server, packet)?
  reply ← ! @udp.recv_from(sock, 5000)?
  ! @udp.close(sock)?

  data ← reply.data
  answers ← byte_at(data, 6) * 256 + byte_at(data, 7)

  first_rr ← 12 + len(qname(host)) + 4
  address ← ipv4_at(data, first_rr + 12)

  // The address itself changes between runs, so what is checked here is its shape.
  ! println("got any " + str(answers > 0))
  ! println("octets  " + str(count(split(address, "."))))
⟧
```

```bash
rite run resolve.rite --allow net=0.0.0.0 --allow net=100.100.100.100
```

```text
query   abcd01000001000000000000076578616d706c6503636f6d0000010001
got any true
octets  4
```

Change `server` to the nameserver in your own `/etc/resolv.conf`, and the grant to
match it.

## What this deliberately does not do

A real resolver handles compression pointers in names, follows `CNAME` chains, reads
`AAAA` and `MX` records, retries, and falls back to TCP when a reply does not fit in
a datagram — which is what [`@tcp`](../book/sockets.md) is for. The parsing here
assumes the first answer is an `A` record and that the name in it is a pointer, both
of which are true for this query and neither of which is true in general.

It is enough to show the shape of the work: build bytes, send one datagram, index
into the reply. Everything else is more of the same.

## Next

- [Network: sockets](../book/sockets.md) — `@udp` and `@tcp` in full
- [Values and atoms](../book/values.md) — the byte builtins
- [Hashing and encoding](../book/crypto.md) — `@crypto`, digests and encodings
