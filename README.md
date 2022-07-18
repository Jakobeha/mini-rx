# mini-rx: Tiny ~~reactive programming~~ *change propagation* a la [scala.rx](https://github.com/lihaoyi/scala.rx)

[Cargo documentation](https://docs.rs/mini-rx)

## Example

```rust
use mini_rx::*;

fn example() {
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
```

## Overview

`mini-rx` is a bare-bones implementation of "reactive programming" in Rust with 1 dependency. It uses manual polling and integrates well with the borrow checker by storing all values in a central data dependency graph, `RxDAG`.

The type of reactive programming is **signal-based**, which is similar to [scala.rx](https://github.com/lihaoyi/scala.rx) but different than most libraries (which are **stream-based**) and maybe not technically FRP. Instead of manipulating a stream of values, you manipulate variables which trigger other computed values to recompute, which in turn trigger other values to recompute and side effects to run.

## Key concepts/types

- `RxDAG`: stores all your `Rx`s. Lifetime rules guarantee that they don't change while you have active references (see [Lifetimes](#lifetimes))
	- `new_var`: creates a `Var`
	- `new_crx`: creates a `CRx`
	- `run_crx`: runs a side-effect, will re-run when any of the accessed `Var`s or `CRx`s change
	- `new_crx[n]`: creates `n` `CRx`s which come from a single computation
	- `recompute`: updates all `Var` and `CRx` values, but requires a mutable reference which ensures there are no active shared references to the old values
	- `now`: recomputes and then gets an `RxContext` so you can get values. It must recompute and thus requires a mutable reference.
	- `stale`: Does not recompute but will not return the most recently set values unless `recompute` was called.
- `Var`: value with no dependencies which you explicitly set, and this triggers updates
- `CRx`: value computed from dependencies
- `RxContext`: allows you to read `Var` and  `CRx` values. Accessible via `RxDAG::now` or in computations (`RxDAG::new_crx`) and side-effects (`RxDAG::run_crx`)
- `MutRxContext`: allows you to write to `Var`s. An `&RxDAG` is a `MutRxContext`. You cannot set values in a `CRx` computation because they are inputs.

## Signal-based

The type of reactive programming is **signal-based**, which is similar to [scala.rx](https://github.com/lihaoyi/scala.rx) but different than most libraries (which are **stream-based**). Instead of manipulating a stream of values, you manipulate variables which trigger other computed values to recompute, which in turn trigger other values to recompute and side effects to run.

You can simulate stream-based reactivity by adding a side-effect which pushes values on trigger, like so:

```rust
use mini_rx::*;

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
```

For more traditional stream-based reactive programming, I recommend [reactive-rs](https://docs.rs/reactive-rs/latest/reactive_rs/)

## Lifetimes

You can't obtain a mutable reference to the value stored within a `Var`. Instead you call `Var::set` or `Var::modify` with a completely new value. This is because there may be active references to the old `Var`. When you call `Var::set` it doesn't immedidately change the old value, so those references won't change.

In order to actually update the reactive values and run side-effects, you must call `RxDAG::recompute`, or a function which internally calls `recompute` like `RxDAG::now`. In order to do this, you need a mutable refernce to the `RxDAG`, which you can only get if there are no active references to any of the reactive values.

Additionally, any compute function in the `RxDAG` must live longer than the `RxDAG` itself. This is because the function may be called any time while the `RxDAG` is alive, when it gets recomputed. So if you have values which you reference in `CRx` computations or side-effects, you must either declare them before the `RxDAG` or use something like a `Weak` reference to ensure that they are still alive when used.

## Why? Signal-based Reactive programming 101

Here's a situation commonly encountered in programming: you have a value `a` which should always equal `b + c`. You don't want `a` to be a function, but when `b` or `c` changes, `a` must be recalculated.

Or here's another situation: you have an action which must always run when a value changes, which sends the updated value to the server.

You can chain these. Perhaps the value you want to send to the server on update is `a`. Perhaps `b` and `c` are computed from other values, `d, e, f`, and so on. Ultimately, for the theoretical folks, you have a DAG (directed-acyclic-graph) of values and dependencies. Modify one of the roots, and it triggers a cascade of computations and side effects.