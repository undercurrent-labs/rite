//! `@random` is random by default and reproducible on request.
//!
//! It used to be neither. `RandomCap::new(42)` was the default, so every `rite run` on
//! every machine drew the identical sequence forever — `@random.int(1, 6)` was a
//! constant. Meanwhile `uuid` bypassed the generator for system entropy, so the one
//! path that *was* meant to be reproducible under `@random.seed(n)` was not.

use rite_caps::random::RandomCap;
use rite_caps::PermissionSet;
use rite_runtime::Value;

fn ints(cap: &mut RandomCap, n: usize) -> Vec<i64> {
    let perms = PermissionSet::allow_all();
    (0..n)
        .map(|_| {
            cap.call(
                "int",
                vec![Value::Int(1), Value::Int(1_000_000_000)],
                &perms,
            )
            .expect("random.int")
            .as_int()
            .expect("an int")
        })
        .collect()
}

fn uuid(cap: &mut RandomCap) -> String {
    cap.call("uuid", vec![], &PermissionSet::allow_all())
        .expect("random.uuid")
        .as_str()
        .expect("a string")
        .to_string()
}

#[test]
fn two_default_generators_do_not_agree() {
    // The regression: both were seeded 42, so this comparison was equality.
    let a = ints(&mut RandomCap::from_entropy(), 8);
    let b = ints(&mut RandomCap::from_entropy(), 8);
    assert_ne!(
        a, b,
        "two fresh generators produced the same sequence, so the default is not random"
    );
}

#[test]
fn an_explicit_seed_reproduces_a_sequence() {
    assert_eq!(
        ints(&mut RandomCap::new(7), 8),
        ints(&mut RandomCap::new(7), 8)
    );
}

#[test]
fn different_seeds_produce_different_sequences() {
    assert_ne!(
        ints(&mut RandomCap::new(7), 8),
        ints(&mut RandomCap::new(8), 8)
    );
}

#[test]
fn reseeding_mid_run_restarts_the_sequence() {
    let perms = PermissionSet::allow_all();
    let mut cap = RandomCap::from_entropy();
    cap.call("seed", vec![Value::Int(99)], &perms)
        .expect("seed");
    let first = ints(&mut cap, 5);
    cap.call("seed", vec![Value::Int(99)], &perms)
        .expect("seed");
    assert_eq!(
        first,
        ints(&mut cap, 5),
        "the same seed replays the sequence"
    );
}

#[test]
fn a_seeded_run_reproduces_its_uuids_too() {
    // `uuid` used to call `Uuid::new_v4()`, which reads system entropy and ignores the
    // seed — so a "deterministic" run produced a different identifier every time.
    assert_eq!(uuid(&mut RandomCap::new(3)), uuid(&mut RandomCap::new(3)));
    assert_ne!(uuid(&mut RandomCap::new(3)), uuid(&mut RandomCap::new(4)));
}

#[test]
fn generated_uuids_are_well_formed_version_4() {
    let mut cap = RandomCap::new(1);
    for _ in 0..16 {
        let u = uuid(&mut cap);
        let parsed = uuid::Uuid::parse_str(&u).unwrap_or_else(|e| panic!("{u}: {e}"));
        assert_eq!(parsed.get_version_num(), 4, "{u} is not a v4 UUID");
        // RFC 4122 variant: the high bits of the 9th byte are `10`.
        assert_eq!(parsed.as_bytes()[8] & 0xC0, 0x80, "{u} has a bad variant");
    }
}

#[test]
fn distinct_uuids_come_out_of_one_generator() {
    let mut cap = RandomCap::new(5);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..64 {
        assert!(seen.insert(uuid(&mut cap)), "a uuid repeated");
    }
}

#[test]
fn random_is_refused_without_the_permission() {
    let mut denied = PermissionSet::default_secure();
    denied.random = false;
    let mut cap = RandomCap::new(1);
    assert!(
        cap.call("int", vec![Value::Int(1), Value::Int(6)], &denied)
            .is_err(),
        "a denied grant must stop the call, seeded or not"
    );
}

#[test]
fn values_stay_inside_the_requested_range() {
    let perms = PermissionSet::allow_all();
    let mut cap = RandomCap::from_entropy();
    for _ in 0..200 {
        let v = cap
            .call("int", vec![Value::Int(1), Value::Int(6)], &perms)
            .expect("int")
            .as_int()
            .expect("int");
        assert!((1..=6).contains(&v), "{v} is outside 1..=6");
    }
}

#[test]
fn an_inverted_range_is_an_error_not_a_panic() {
    let mut cap = RandomCap::new(1);
    assert!(cap
        .call(
            "int",
            vec![Value::Int(10), Value::Int(1)],
            &PermissionSet::allow_all()
        )
        .is_err());
}

#[test]
fn shuffle_and_choose_follow_the_seed() {
    let perms = PermissionSet::allow_all();
    let list = || Value::list((1..=12).map(Value::Int).collect::<Vec<_>>());

    let mut a = RandomCap::new(11);
    let mut b = RandomCap::new(11);
    let sa = a.call("shuffle", vec![list()], &perms).expect("shuffle");
    let sb = b.call("shuffle", vec![list()], &perms).expect("shuffle");
    assert!(sa.structural_eq(&sb), "the same seed must shuffle alike");

    let ca = a.call("choose", vec![list()], &perms).expect("choose");
    let cb = b.call("choose", vec![list()], &perms).expect("choose");
    assert!(ca.structural_eq(&cb));
}

#[test]
fn shuffle_keeps_every_element() {
    let perms = PermissionSet::allow_all();
    let mut cap = RandomCap::from_entropy();
    let input: Vec<Value> = (1..=20).map(Value::Int).collect();
    let out = cap
        .call("shuffle", vec![Value::list(input.clone())], &perms)
        .expect("shuffle");
    let Value::List(xs) = out else {
        panic!("shuffle returned a non-list")
    };
    let mut got: Vec<i64> = xs.iter().filter_map(|v| v.as_int()).collect();
    got.sort_unstable();
    assert_eq!(
        got,
        (1..=20).collect::<Vec<_>>(),
        "shuffle lost or added elements"
    );
}

#[test]
fn choose_on_an_empty_list_is_none_rather_than_a_panic() {
    let mut cap = RandomCap::new(1);
    let got = cap
        .call(
            "choose",
            vec![Value::list(Vec::<Value>::new())],
            &PermissionSet::allow_all(),
        )
        .expect("choose");
    assert!(matches!(got, Value::None));
}
