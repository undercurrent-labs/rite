# Sugar pack

Rite is a **sigil language**: dense glyphs with ASCII twins, all desugaring into the same IR. This chapter is the map of the sugar pack.

## Quick table

| Glyph / form | ASCII / form | Meaning |
|--------------|--------------|---------|
| `1..10` | `1..10` | Exclusive range → list |
| `1..=10` / `1‥10` | `1..=10` | Inclusive range |
| `→ rest` / `→ tail` | `-> rest` | Drop first element |
| `→ take(n)` / `→ drop(n)` | same | Prefix / suffix skip |
| `→ .field` | same | Map records to field |
| `→ words` / `→ lines` / `→ join(s)` | same | String stages |
| `if … else …` | `else` keyword | Else branch (also `:`) |
| `¿` / `unless` | `unless` | Negated if |
| `∀ x ∈ xs ⟦⟧` | `for x in xs [[]]` | For-each |
| `loop n ⟦⟧` | `loop n [[]]` | Repeat n times |
| `while c ⟦⟧` | `while c [[]]` | While loop |
| `c += 1` | same | Op-assign (`-=` `*=` `/=` `%=`) |
| `2 ** 8` / `pow` | `**` | Power |
| `7 ÷ 2` / `idiv` | `idiv(7,2)` | Integer division |
| `∧ ∨ ¬ ⊻` | `and or not xor` | Logic |
| `✓ v` / `✗ e` | `ok(v)` / `err(e)` | Result marks |
| `¶ x` | `say x` | Print line |
| `⊏` | `use` | Import / HTTP middleware plug-in |
| `f ∘ g` | `compose(f, g)` | Function compose |
| `abs` `clamp` `concat` `repeat` | same | Numeric / list helpers |
| `unwrap_or` `is_ok` `is_err` `or_else` | same | Result helpers |

## Ranges

```rite browser
(1..5) → sum      // 10  (1+2+3+4)
(1..=5) → sum     // 15
(0‥3) → count     // 4
```

## Pipeline stages

```rite browser
[1, 2, 3, 4] → rest → take(2) → reverse → sum
```

```rite browser
[⟨score: 1⟩, ⟨score: 2⟩] → .score → sum
```

```rite browser
"a b c" → words → count
```

```rite browser
["a", "b"] → join("-")
```

List “spread” is written with **`concat`** (literal `..` inside lists is reserved for match rest / ranges):

```rite browser
concat([1], [2, 3], [4]) → sum   // 10
```

Record update uses **merge** (`+`):

```rite browser
base ← ⟨a: 1, b: 2⟩
base + ⟨b: 9, c: 3⟩              // ⟨a: 1, b: 9, c: 3⟩
```

## Control flow

```rite
// else keyword (ASCII) or colon (both dialects)
if x > 0 [[ "pos" ]] else [[ "nonpos" ]]
? x > 0 ⟦ "pos" ⟧ : ⟦ "nonpos" ⟧

unless ready ⟦
  say "waiting"
⟧

s ↢ 0
for n in 1..5 ⟦
  s += n
⟧

∀ n ∈ xs ⟦
  ¶ n
⟧

c ↢ 0
while c < 10 ⟦
  c += 1
⟧

loop 3 ⟦
  say "tick"
⟧
```

## Numbers and logic

```rite browser
2 ** 10          // 1024
7 ÷ 2            // 3
abs(-5)
clamp(15, 0, 10)
true ⊻ false
```

## Results

```rite browser
✓ 200            // ok(200)
✗ "fail"         // err("fail")
unwrap_or(err(1), 99)
is_ok(ok(1))
```

## Print

```rite browser
say "hello"
¶ 42
! @console.println("full form still works")
```

## Compose

```rite browser
◆ double(n) ⟦ ^ n * 2 ⟧
◆ inc(n) ⟦ ^ n + 1 ⟧
f ← double ∘ inc
f(3)   // 8
```

## Dual dialect

Every glyph form has an ASCII spelling. Use:

```bash
rite fmt --ascii script.rite
rite convert script.rite --to glyph --stdout
```

See also: [Pipelines](pipelines.md), [Functions](functions.md), [Results](results.md), [Collections](collections.md).
