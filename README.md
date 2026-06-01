# lau-state-machine

**Hierarchical state machine for agents and game entities — deterministic transitions, guards, actions.**

A Rust library implementing a hierarchical finite state machine (HFSM) with priority-based transition selection, named guard functions, a fluent builder API, and a pre-built `AgentStateMachine` for game NPCs. Full serde support, 42 tests, zero runtime dependencies beyond serde.

---

## What This Does

`lau-state-machine` provides:

1. **A `StateMachine`** with hierarchical states — child states inherit transitions from parent states, enabling "any state → Flee" patterns without enumerating every source state.
2. **Priority-based conflict resolution** — when multiple transitions match, the highest-priority one wins.
3. **Named guard functions** — transitions can be conditional on `GuardEvaluator` predicates that inspect event data.
4. **A fluent builder** (`StateMachineBuilder`) for declaratively constructing machines.
5. **A pre-built `AgentStateMachine`** with 8 states (Idle, Wander, FollowPlayer, Flee, Interact, Work, Rest, Alert) and standard game-agent transitions.
6. **Full serialization** — the entire machine state (current state, history, tick count) round-trips through JSON.

---

## Key Idea

In a flat state machine, adding a "danger → Flee" transition requires a separate transition from *every* state. In a hierarchical machine, you define the transition once on a parent state, and all children inherit it. This crate implements **hierarchical event processing**: when an event arrives, the machine checks transitions on the current state *and all ancestor states*, collecting matches, then sorting by priority and evaluating guards.

```
           Move (parent)
          /     \
       Run      Walk
       
  Event "stop" defined on Move → matches from Run or Walk
  Event "danger" defined on root → matches from anywhere
```

---

## Install

```toml
[dependencies]
lau-state-machine = "0.1.0"
```

```sh
cargo add lau-state-machine
```

### Requirements

- Rust 2021 edition
- `serde` 1.x + `serde_json` 1.x

---

## Quick Start

### Build a state machine

```rust
use lau_state_machine::*;

let mut sm = StateMachineBuilder::new()
    .state("Idle")
        .on_enter("log_idle")
        .on_exit("log_leave_idle")
    .state("Patrol")
        .on_enter("start_patrol_route")
    .state("Attack")
        .on_enter("draw_weapon")
    .state("Flee")
        .on_enter("run_away")
    // Transitions
    .transition("Idle", "Patrol", "enemy_spotted")
    .transition("Patrol", "Attack", "in_range")
    .transition("Attack", "Flee", "low_health")
        .priority(10)
    .transition("Flee", "Idle", "safe")
    .build();

sm.start(StateId::new("Idle")).unwrap();
```

### Process events

```rust
// Idle → Patrol
let next = sm.process_event(Event::new("enemy_spotted"));
assert_eq!(next, Some(StateId::new("Patrol")));

// Patrol → Attack
let next = sm.process_event(Event::new("in_range"));
assert_eq!(next, Some(StateId::new("Attack")));

// Check history
assert_eq!(sm.history(), &[
    StateId::new("Idle"),
    StateId::new("Patrol"),
    StateId::new("Attack"),
]);
```

### Guards with event data

```rust
let mut sm = StateMachineBuilder::new()
    .state("Idle")
    .state("Run")
    .transition("Idle", "Run", "go")
        .guard("check_energy")
    .build();
sm.start(StateId::new("Idle")).unwrap();

let mut guards = GuardEvaluator::new();
guards.register("check_energy", |e| {
    e.data.get("energy").copied().unwrap_or(0.0) > 5.0
});

// Guard fails — energy too low
let next = sm.process_event_with_guards(Event::new("go").with_data("energy", 2.0), &guards);
assert!(next.is_none());

// Guard passes
let next = sm.process_event_with_guards(Event::new("go").with_data("energy", 10.0), &guards);
assert_eq!(next, Some(StateId::new("Run")));
```

### Hierarchical states

```rust
let sm = StateMachineBuilder::new()
    .state("Move")              // parent
    .state("Run").parent("Move")   // child
    .state("Walk").parent("Move")  // child
    .state("Idle")
    // Transition on parent → inherited by children
    .transition("Move", "Idle", "stop")
    .build();

// When in "Run", event "stop" matches via parent "Move"
```

### Pre-built AgentStateMachine

```rust
let mut sm = AgentStateMachine::new();
sm.start(StateId::new("Idle")).unwrap();

sm.process_event(Event::new("timer"));           // Idle → Wander
sm.process_event(Event::new("danger"));           // Wander → Flee
sm.process_event(Event::new("safe"));             // Flee → Idle
```

### Serialize state

```rust
let json = serde_json::to_string(&sm).unwrap();
let restored: StateMachine = serde_json::from_str(&json).unwrap();
assert_eq!(sm.current_id(), restored.current_id());
```

---

## API Reference

### Core Types

| Type | Description |
|---|---|
| `StateId` | Newtype wrapper around `String` identifying a state |
| `Event` | Named event with optional `HashMap<String, f64>` data payload |
| `Transition` | Directed edge: `from → to` on event name, optional guard, priority |
| `State` | Node: id, optional `on_enter`/`on_exit`/`on_update` action names, optional parent, children |
| `StateMachine` | The machine: states, transitions, current state, history, tick counter |
| `StateMachineBuilder` | Fluent builder for constructing machines |
| `GuardEvaluator` | Registry of named guard functions `Fn(&Event) -> bool` |
| `AgentStateMachine` | Factory for a pre-built game-agent machine |

### StateMachine Methods

- `new()`, `add_state()`, `add_transition()`
- `start(initial)` → sets current state, clears history
- `process_event(event)` → finds matching transition (skips guarded), returns new state
- `process_event_with_guards(event, guards)` → evaluates guards
- `current_state()`, `current_id()` → inspect current state
- `is_in_state(state)` → true if current state *is* or *descends from* the given state
- `history()` → slice of visited states
- `tick()`, `tick_count()` → manual tick counter
- `available_transitions()` → transitions from current state
- `reset()` → clear current state, history, and ticks

### StateMachineBuilder Methods

Chainable: `.state("name")`, `.parent("parent")`, `.on_enter("action")`, `.on_exit("action")`, `.on_update("action")`, `.transition("from", "to", "event")`, `.guard("name")`, `.priority(n)`, `.build()`.

### GuardEvaluator

- `register(name, fn)` — register a guard function
- `evaluate(name, event)` — run a guard; unknown guards return `true` (permissive)

---

## How It Works

### Hierarchical Event Processing

When `process_event` is called:

1. Start at the current state.
2. Collect all transitions matching `(current_state, event_name)`.
3. Walk up to the parent state, collect matching transitions there too.
4. Continue up the ancestor chain until reaching a root state.
5. Sort all collected transitions by priority (descending).
6. Return the first transition whose guard passes (or has no guard).

This means a transition defined on a parent state fires from any descendant state — the core of hierarchical state machines.

### Priority Resolution

Transitions have a `priority: u32` field (default 0). When multiple transitions match:

```
matching.sort_by(|a, b| b.priority.cmp(&a.priority));
```

The first transition in the sorted list whose guard passes wins. This allows "override" patterns — e.g., a low-priority "Idle → Wander" transition and a high-priority "any → Flee" on `danger`.

### Guard Evaluation

Guards are **named strings** stored on transitions. Two evaluation paths exist:

- `process_event()` — skips any transition with a guard (treats as non-matching).
- `process_event_with_guards()` — looks up the guard name in the `GuardEvaluator` and calls the registered function.

Unknown guard names default to `true` (permissive), so adding a new guard to the machine doesn't break existing callers until they register it.

### State Hierarchy

States have an optional `parent: Option<StateId>` and `children: Vec<StateId>`. The builder wires up bidirectional links automatically. `is_in_state()` walks up the ancestor chain to check membership.

---

## The Math

### Transition Selection as a Priority Queue

Given a set of matching transitions `T = {t₁, t₂, ..., tₙ}` with priorities `p₁, p₂, ..., pₙ` and guards `g₁, g₂, ..., gₙ`:

```
selected = first tᵢ in sort_desc(T, by=pᵢ) where gᵢ(event) = true
```

This is a **greedy** selection: the highest-priority transition whose guard passes wins. There is no backtracking or cost optimization.

### Hierarchical Matching as Ancestor Traversal

For a state `s` with ancestor chain `s → p₁ → p₂ → ... → root`:

```
matches = ∪ᵢ { t ∈ transitions | t.from = stateᵢ ∧ t.event = event_name }
```

The set union is collected in order from leaf to root, then sorted globally by priority. This gives leaf states "first shot" at handling events, but priority can override this ordering.

### Tick Counter

A simple monotonically increasing counter: `tick ← tick + 1`. Useful for time-based guards or periodic actions. No automatic ticking — the caller calls `tick()` when appropriate.

---

## License

MIT
