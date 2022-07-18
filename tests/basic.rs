use mini_rx::*;
use test_log::test;
use std::cell::{Cell, RefCell};

#[test]
fn test_rx() {
    let side_effect = Cell::new(1);

    let mut g = RxDAG::new();
    let rx = g.new_var(1);
    let crx = g.new_crx(move |g| *rx.get(g) * 2);
    let side_effect_ref = &side_effect;
    g.run_crx(move |g| {
        side_effect_ref.set(side_effect_ref.get() + *crx.get(g));
    });
    assert_eq!(rx.get(g.stale()), &1);
    assert_eq!(crx.get(g.stale()), &2);
    assert_eq!(side_effect.get(), 3);

    rx.set(&g, 2);
    assert_eq!(rx.get(g.stale()), &1);
    assert_eq!(crx.get(g.stale()), &2);
    assert_eq!(side_effect.get(), 3);
    g.recompute();
    assert_eq!(rx.get(g.stale()), &2);
    assert_eq!(crx.get(g.stale()), &4);
    assert_eq!(side_effect.get(), 7);

    rx.set(&g, 4);
    assert_eq!(rx.get(g.stale()), &2);
    assert_eq!(crx.get(g.stale()), &4);
    assert_eq!(side_effect.get(), 7);
    g.recompute();
    assert_eq!(rx.get(g.stale()), &4);
    assert_eq!(crx.get(g.stale()), &8);
    assert_eq!(side_effect.get(), 15);
}

#[test]
fn test_rx_modify() {
    let mut g = RxDAG::new();
    let rx = g.new_var(1);
    let crx = g.new_crx(move |g| *rx.get(g) * 2);
    let side_effect = Cell::new(1);
    let side_effect2 = &side_effect;
    g.run_crx(move |g| {
        side_effect2.set(side_effect2.get() + crx.get(g));
    });
    assert_eq!(rx.get(g.now()), &1);
    assert_eq!(crx.get(g.now()), &2);
    assert_eq!(side_effect.get(), 3);

    rx.modify(&g, |g| g + 3);
    rx.modify(&g, |g| g + 5);
    assert_eq!(rx.get(g.now()), &9);
    assert_eq!(crx.get(g.now()), &18);
    assert_eq!(side_effect.get(), 21);

    // See above
    drop(g);
}

#[test]
fn test_rx_multiple_inputs_outputs() {
    let mut g = RxDAG::new();
    let rx = g.new_var(1);
    let rx2 = g.new_var(2);
    let rx3 = g.new_var(vec![3, 4]);
    {
        let crx = g.new_crx(move |g| vec![*rx.get(g) * 10, *rx2.get(g) * 10]);
        let crx2 = g.new_crx(move |g| {
            let mut vec = Vec::new();
            vec.push(*rx.get(g));
            for elem in rx3.get(g).iter().copied() {
                vec.push(elem)
            }
            for elem in crx.get(g).iter().copied() {
                vec.push(elem)
            }
            vec
        });
        let (crx3_1, crx3_2) = g.new_crx2(move |g| {
            let vec = crx.get(g);
            (vec[0] * 10, vec[1] * 10)
        });
        let (crx4_1, crx4_2, crx4_3) = g.new_crx3(move |g| {
            let v2 = *rx.get(g);
            let v3 = *crx3_2.get(g);
            let v4 = rx3.get(g)[0];
            (v2, v3, v4 * 100)
        });

        assert_eq!(rx.get(g.now()), &1);
        assert_eq!(rx2.get(g.now()), &2);
        assert_eq!(rx3.get(g.now()), &vec![3, 4]);
        assert_eq!(crx.get(g.now()), &vec![10, 20]);
        assert_eq!(crx2.get(g.now()), &vec![1, 3, 4, 10, 20]);
        assert_eq!(crx3_1.get(g.now()), &100);
        assert_eq!(crx3_2.get(g.now()), &200);
        assert_eq!(crx4_1.get(g.now()), &1);
        assert_eq!(crx4_2.get(g.now()), &200);
        assert_eq!(crx4_3.get(g.now()), &300);

        rx.set(&g, 5);
        rx2.set(&g, 6);
        rx3.set(&g, vec![7, 8, 9]);
        g.recompute();

        assert_eq!(rx.get(g.now()), &5);
        assert_eq!(rx2.get(g.now()), &6);
        assert_eq!(rx3.get(g.now()), &vec![7, 8, 9]);
        assert_eq!(crx.get(g.now()), &vec![50, 60]);
        assert_eq!(crx2.get(g.now()), &vec![5, 7, 8, 9, 50, 60]);
        assert_eq!(crx3_1.get(g.now()), &500);
        assert_eq!(crx3_2.get(g.now()), &600);
        assert_eq!(crx4_1.get(g.now()), &5);
        assert_eq!(crx4_2.get(g.now()), &600);
        assert_eq!(crx4_3.get(g.now()), &700);
    }
}

#[test]
fn test_drx_split() {
    let mut g = RxDAG::new();
    let rx = g.new_var(vec![1, 2, 3]);
    {
        let drx0 = rx.derive_using_clone(|x| &x[0], |x, new| {
            x[0] = new;
        });
        let drx1 = rx.derive_using_clone(|x| &x[1], |x, new| {
            x[1] = new;
        });
        let drx2 = rx.derive_using_clone(|x| &x[2], |x, new| {
            x[2] = new;
        });
        assert_eq!(drx0.get(g.now()), &1);
        assert_eq!(drx1.get(g.now()), &2);
        assert_eq!(drx2.get(g.now()), &3);
        drx0.set(&g, 2);
        drx1.set(&g, 3);
        drx2.set(&g, 4);
    }
    assert_eq!(rx.get(g.now()), &vec![2, 3, 4]);
}

#[test]
fn test_crx() {
    let mut g = RxDAG::new();
    let rx = g.new_var(vec![1, 2, 3]);
    {
        let crx = g.new_crx(move |g| rx.get(g)[0] * 2);
        let crx2 = g.new_crx(move |g| *crx.get(g) + rx.get(g)[1] * 10);
        let crx3 = g.new_crx(move |g| crx2.get(g).to_string());
        assert_eq!(*crx.get(g.now()), 2);
        assert_eq!(*crx2.get(g.now()), 22);
        assert_eq!(&*crx3.get(g.now()), "22");
        rx.set(&g, vec![2, 3, 4]);
        assert_eq!(*crx.get(g.now()), 4);
        assert_eq!(*crx2.get(g.now()), 34);
        assert_eq!(&*crx3.get(g.now()), "34");
        rx.set(&g, vec![3, 4, 5]);
        assert_eq!(*crx.get(g.now()), 6);
        assert_eq!(*crx2.get(g.now()), 46);
        assert_eq!(&*crx3.get(g.now()), "46");
    }
}

#[test]
fn test_readme() {
    // setup
    let side_effect = Cell::new(0);
    let side_effect2 = RefCell::new(String::new());

    // The centralized data dependency graph
    let mut g = RxDAG::new();

    // Create variables which you can set
    let var1 = g.new_var(1);
    let var2 = g.new_var("hello");
    assert_eq!(var1.get(g.now()), &1);
    assert_eq!(var2.get(g.now()), &"hello");
    var1.set(&g, 2);
    var2.set(&g, "world");
    assert_eq!(var1.get(g.now()), &2);
    assert_eq!(var2.get(g.now()), &"world");

    // Create computed values which depend on these variables...
    let crx1 = g.new_crx(move |g| var1.get(g) * 2);
    // ...and other Rxs
    let crx2 = g.new_crx(move |g| format!("{}-{}", var2.get(g), crx1.get(g) * 2));
    // ...and create multiple values which are computed from a single function
    let (crx3, crx4) = g.new_crx2(move |g| var2.get(g).split_at(3));
    assert_eq!(crx1.get(g.now()), &4);
    assert_eq!(crx2.get(g.now()), &"world-8");
    assert_eq!(crx3.get(g.now()), &"wor");
    assert_eq!(crx4.get(g.now()), &"ld");
    var1.set(&g, 3);
    var2.set(&g, &"rust");
    assert_eq!(crx1.get(g.now()), &6);
    assert_eq!(crx2.get(g.now()), &"rust-12");
    assert_eq!(crx3.get(g.now()), &"rus");
    assert_eq!(crx4.get(g.now()), &"t");

    // Run side effects when a value is recomputed
    let var3 = g.new_var(Vec::from("abc"));
    let side_effect_ref = &side_effect;
    let side_effect_ref2 = &side_effect2;
    // borrowed values must outlive g but don't have to be static
    g.run_crx(move |g| {
        side_effect_ref.set(side_effect_ref.get() + var1.get(g));
        side_effect_ref2.borrow_mut().push_str(&String::from_utf8_lossy(var3.get(g)));
    });
    assert_eq!(side_effect.get(), 3);
    assert_eq!(&*side_effect2.borrow(), &"abc");
    var1.set(&g, 4);
    g.recompute();

    assert_eq!(side_effect.get(), 7);
    assert_eq!(&*side_effect2.borrow(), &"abcabc");

    // Note that the dependencies aren't updated until .recompute or .now is called...
    var3.set(&g, Vec::from("xyz"));
    assert_eq!(side_effect.get(), 7);
    assert_eq!(&*side_effect2.borrow(), &"abcabc");
    g.recompute();
    assert_eq!(side_effect.get(), 11);
    assert_eq!(&*side_effect2.borrow(), &"abcabcxyz");

    // the side-effect also doesn't trigger when none of its dependencies change
    var2.set(&g, "rust-lang");
    g.recompute();
    assert_eq!(side_effect.get(), 11);
    assert_eq!(&*side_effect2.borrow(), &"abcabcxyz");
    assert_eq!(crx2.get(g.now()), &"rust-lang-16");

    // lastly we can create derived values which will access or mutate part of the base value
    // which are useful to pass to children
    let dvar = var3.derive_using_clone(|x| &x[0], |x, char| {
        x[0] = char;
    });
    assert_eq!(dvar.get(g.now()), &b'x');
    dvar.set(&g, b'b');
    assert_eq!(dvar.get(g.now()), &b'b');
    assert_eq!(var3.get(g.now()), &b"byz");
    dvar.set(&g, b'f');
    assert_eq!(dvar.get(g.now()), &b'f');
    assert_eq!(var3.get(g.now()), &b"fyz");
    assert_eq!(&*side_effect2.borrow(), &"abcabcxyzbyzfyz");
}

#[test]
fn stream_like() {
    let stream = RefCell::new(Vec::new());
    let stream_ref = &stream;
    let input1 = vec![1, 2, 3];
    let input2 = vec![0.5, 0.4, 0.8];

    let mut g = RxDAG::new();
    let var1 = g.new_var(0);
    let var2 = g.new_var(0.0);
    let crx = g.new_crx(move |g| *var1.get(g) as f64 + *var2.get(g));

    g.run_crx(move |g| {
        stream_ref.borrow_mut().push(*crx.get(g));
    });

    assert_eq!(&*stream.borrow(), &vec![0.0]);
    for (a, b) in input1.iter().zip(input2.iter()) {
        var1.set(&g, *a);
        var2.set(&g, *b);
        g.recompute();
    }
    assert_eq!(&*stream.borrow(), &vec![0.0, 1.5, 2.4, 3.8]);
}