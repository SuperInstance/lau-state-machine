use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// StateId
// ---------------------------------------------------------------------------

/// Newtype wrapper around a `String` identifying a state.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateId(pub String);

impl StateId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for StateId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

/// An event that may trigger a state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub data: HashMap<String, f64>,
}

impl Event {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data: HashMap::new(),
        }
    }

    pub fn with_data(mut self, key: impl Into<String>, value: f64) -> Self {
        self.data.insert(key.into(), value);
        self
    }
}

// ---------------------------------------------------------------------------
// Transition
// ---------------------------------------------------------------------------

/// A directed transition between two states, triggered by a named event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from: StateId,
    pub to: StateId,
    pub event: String,
    pub guard: Option<String>,
    pub priority: u32,
}

impl Transition {
    pub fn new(
        from: impl Into<StateId>,
        to: impl Into<StateId>,
        event: impl Into<String>,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            event: event.into(),
            guard: None,
            priority: 0,
        }
    }

    /// Check whether this transition applies to the given current state and event.
    pub fn matches(&self, from: &StateId, event: &Event) -> bool {
        &self.from == from && self.event == event.name
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A node in a hierarchical state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub id: StateId,
    pub on_enter: Option<String>,
    pub on_exit: Option<String>,
    pub on_update: Option<String>,
    pub parent: Option<StateId>,
    pub children: Vec<StateId>,
}

impl State {
    pub fn new(id: impl Into<StateId>) -> Self {
        Self {
            id: id.into(),
            on_enter: None,
            on_exit: None,
            on_update: None,
            parent: None,
            children: Vec::new(),
        }
    }

    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

// ---------------------------------------------------------------------------
// StateMachine
// ---------------------------------------------------------------------------

/// Core hierarchical state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    pub states: HashMap<StateId, State>,
    pub transitions: Vec<Transition>,
    pub current: Option<StateId>,
    pub history: Vec<StateId>,
    pub tick: u64,
}

impl StateMachine {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            transitions: Vec::new(),
            current: None,
            history: Vec::new(),
            tick: 0,
        }
    }

    pub fn add_state(&mut self, state: State) {
        self.states.insert(state.id.clone(), state);
    }

    pub fn add_transition(&mut self, transition: Transition) {
        self.transitions.push(transition);
    }

    /// Start the machine at `initial`. Fails if the state does not exist.
    pub fn start(&mut self, initial: StateId) -> Result<(), String> {
        if !self.states.contains_key(&initial) {
            return Err(format!("state '{}' does not exist", initial));
        }
        self.current = Some(initial.clone());
        self.history.clear();
        self.history.push(initial);
        self.tick = 0;
        Ok(())
    }

    /// Process an event: find the highest-priority matching transition and take it.
    /// Returns the new state id if a transition fired, `None` otherwise.
    pub fn process_event(&mut self, event: Event) -> Option<StateId> {
        let current_id = self.current.as_ref()?;

        // Collect matching transitions, also check ancestor states (hierarchical).
        let mut matching: Vec<&Transition> = Vec::new();
        let mut candidate = Some(current_id.clone());

        while let Some(ref sid) = candidate {
            for t in &self.transitions {
                if t.matches(sid, &event) {
                    matching.push(t);
                }
            }
            candidate = self
                .states
                .get(sid)
                .and_then(|s| s.parent.clone());
        }

        // Sort by priority descending (highest priority first).
        matching.sort_by_key(|t| std::cmp::Reverse(t.priority));

        // We need a guard evaluator to actually check guards – but the machine itself
        // only stores guard names. If a guard exists and cannot be evaluated here,
        // we skip it (callers should use `process_event_with_guards` for guarded transitions).
        // For `process_event` we treat all guards as passing.
        let chosen = matching.into_iter().find(|t| t.guard.is_none());

        if let Some(t) = chosen {
            self.current = Some(t.to.clone());
            self.history.push(t.to.clone());
            return Some(t.to.clone());
        }

        None
    }

    /// Process an event using a `GuardEvaluator` to check named guards.
    pub fn process_event_with_guards(
        &mut self,
        event: Event,
        guards: &GuardEvaluator,
    ) -> Option<StateId> {
        let current_id = self.current.as_ref()?;

        let mut matching: Vec<&Transition> = Vec::new();
        let mut candidate = Some(current_id.clone());

        while let Some(ref sid) = candidate {
            for t in &self.transitions {
                if t.matches(sid, &event) {
                    matching.push(t);
                }
            }
            candidate = self
                .states
                .get(sid)
                .and_then(|s| s.parent.clone());
        }

        matching.sort_by_key(|t| std::cmp::Reverse(t.priority));

        let chosen = matching.into_iter().find(|t| {
            match &t.guard {
                None => true,
                Some(guard_name) => guards.evaluate(guard_name, &event),
            }
        });

        if let Some(t) = chosen {
            self.current = Some(t.to.clone());
            self.history.push(t.to.clone());
            return Some(t.to.clone());
        }

        None
    }

    pub fn current_state(&self) -> Option<&State> {
        self.current.as_ref().and_then(|id| self.states.get(id))
    }

    pub fn current_id(&self) -> Option<StateId> {
        self.current.clone()
    }

    /// Returns true if the current state *is* `state` or is a child of `state`.
    pub fn is_in_state(&self, state: &StateId) -> bool {
        let mut candidate = self.current.as_ref().cloned();
        while let Some(ref sid) = candidate {
            if sid == state {
                return true;
            }
            candidate = self.states.get(sid).and_then(|s| s.parent.clone());
        }
        false
    }

    pub fn history(&self) -> &[StateId] {
        &self.history
    }

    pub fn tick(&mut self) {
        self.tick += 1;
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn reset(&mut self) {
        self.current = None;
        self.history.clear();
        self.tick = 0;
    }

    /// All transitions that could fire from the current state (ignoring guards).
    pub fn available_transitions(&self) -> Vec<&Transition> {
        match &self.current {
            Some(id) => self
                .transitions
                .iter()
                .filter(|t| &t.from == id)
                .collect(),
            None => Vec::new(),
        }
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// StateMachineBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for constructing a `StateMachine`.
pub struct StateMachineBuilder {
    machine: StateMachine,
    // Temporary state being built.
    pending_state: Option<State>,
    // Temporary transition being built.
    pending_transition: Option<Transition>,
}

impl StateMachineBuilder {
    pub fn new() -> Self {
        Self {
            machine: StateMachine::new(),
            pending_state: None,
            pending_transition: None,
        }
    }

    /// Begin defining a state with the given id.
    pub fn state(&mut self, id: &str) -> &mut Self {
        self.flush_state();
        self.flush_transition();
        self.pending_state = Some(State::new(id));
        self
    }

    /// Set the parent of the pending state.
    pub fn parent(&mut self, parent_id: &str) -> &mut Self {
        if let Some(s) = &mut self.pending_state {
            s.parent = Some(StateId::new(parent_id));
        }
        self
    }

    /// Set the on_enter action name for the pending state.
    pub fn on_enter(&mut self, action: &str) -> &mut Self {
        if let Some(s) = &mut self.pending_state {
            s.on_enter = Some(action.to_string());
        }
        self
    }

    /// Set the on_exit action name for the pending state.
    pub fn on_exit(&mut self, action: &str) -> &mut Self {
        if let Some(s) = &mut self.pending_state {
            s.on_exit = Some(action.to_string());
        }
        self
    }

    /// Set the on_update action name for the pending state.
    pub fn on_update(&mut self, action: &str) -> &mut Self {
        if let Some(s) = &mut self.pending_state {
            s.on_update = Some(action.to_string());
        }
        self
    }

    /// Begin defining a transition.
    pub fn transition(&mut self, from: &str, to: &str, event: &str) -> &mut Self {
        self.flush_state();
        self.flush_transition();
        self.pending_transition = Some(Transition::new(from, to, event));
        self
    }

    /// Set the guard name on the pending transition.
    pub fn guard(&mut self, name: &str) -> &mut Self {
        if let Some(t) = &mut self.pending_transition {
            t.guard = Some(name.to_string());
        }
        self
    }

    /// Set the priority on the pending transition.
    pub fn priority(&mut self, p: u32) -> &mut Self {
        if let Some(t) = &mut self.pending_transition {
            t.priority = p;
        }
        self
    }

    /// Build and return the final `StateMachine`.
    pub fn build(&mut self) -> StateMachine {
        self.flush_state();
        self.flush_transition();
        // Wire up parent-child relationships.
        let mut parent_links: Vec<(StateId, StateId)> = Vec::new();
        for state in self.machine.states.values() {
            if let Some(ref parent) = state.parent {
                parent_links.push((parent.clone(), state.id.clone()));
            }
        }
        for (parent, child) in &parent_links {
            if let Some(p) = self.machine.states.get_mut(parent) {
                if !p.children.contains(child) {
                    p.children.push(child.clone());
                }
            }
        }
        std::mem::take(&mut self.machine)
    }

    fn flush_state(&mut self) {
        if let Some(s) = self.pending_state.take() {
            self.machine.add_state(s);
        }
    }

    fn flush_transition(&mut self) {
        if let Some(t) = self.pending_transition.take() {
            self.machine.add_transition(t);
        }
    }
}

impl Default for StateMachineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GuardEvaluator
// ---------------------------------------------------------------------------

type GuardFn = Box<dyn Fn(&Event) -> bool>;

/// Stores named guard functions that can be evaluated against an `Event`.
pub struct GuardEvaluator {
    guards: HashMap<String, GuardFn>,
}

impl GuardEvaluator {
    pub fn new() -> Self {
        Self {
            guards: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, f: impl Fn(&Event) -> bool + 'static) {
        self.guards.insert(name.to_string(), Box::new(f));
    }

    /// Evaluate a named guard. Returns `true` if the guard is unknown (permissive default).
    pub fn evaluate(&self, name: &str, event: &Event) -> bool {
        match self.guards.get(name) {
            Some(f) => f(event),
            None => true,
        }
    }
}

impl Default for GuardEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// AgentStateMachine
// ---------------------------------------------------------------------------

/// Pre-built state machine for game agents.
pub struct AgentStateMachine;

impl AgentStateMachine {
    /// Build an `AgentStateMachine` with standard game-agent states and transitions.
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> StateMachine {
        StateMachineBuilder::new()
            .state("Idle")
            .on_enter("on_idle_enter")
            .on_exit("on_idle_exit")
            .on_update("on_idle_update")
            .state("Wander")
            .on_enter("on_wander_enter")
            .on_exit("on_wander_exit")
            .state("FollowPlayer")
            .on_enter("on_follow_enter")
            .on_exit("on_follow_exit")
            .state("Flee")
            .on_enter("on_flee_enter")
            .on_exit("on_flee_exit")
            .state("Interact")
            .on_enter("on_interact_enter")
            .on_exit("on_interact_exit")
            .state("Work")
            .on_enter("on_work_enter")
            .on_exit("on_work_exit")
            .state("Rest")
            .on_enter("on_rest_enter")
            .on_exit("on_rest_exit")
            .state("Alert")
            .on_enter("on_alert_enter")
            .on_exit("on_alert_exit")
            // Standard transitions
            .transition("Idle", "Wander", "timer")
            .transition("Wander", "Idle", "tired")
            .transition("Idle", "FollowPlayer", "player_near")
            .transition("FollowPlayer", "Interact", "reached")
            // Any→Flee via high-priority "danger" from each state
            .transition("Idle", "Flee", "danger")
            .priority(10)
            .transition("Wander", "Flee", "danger")
            .priority(10)
            .transition("FollowPlayer", "Flee", "danger")
            .priority(10)
            .transition("Interact", "Flee", "danger")
            .priority(10)
            .transition("Work", "Flee", "danger")
            .priority(10)
            .transition("Rest", "Flee", "danger")
            .priority(10)
            .transition("Alert", "Flee", "danger")
            .priority(10)
            .transition("Flee", "Idle", "safe")
            .transition("Work", "Rest", "done")
            .transition("Rest", "Idle", "rested")
            // Any→Alert via "conservation_error"
            .transition("Idle", "Alert", "conservation_error")
            .transition("Wander", "Alert", "conservation_error")
            .transition("FollowPlayer", "Alert", "conservation_error")
            .transition("Interact", "Alert", "conservation_error")
            .transition("Work", "Alert", "conservation_error")
            .transition("Rest", "Alert", "conservation_error")
            .transition("Flee", "Alert", "conservation_error")
            .build()
    }
}

impl Default for AgentStateMachine {
    fn default() -> Self {
        Self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- StateId tests --

    #[test]
    fn state_id_new() {
        let id = StateId::new("idle");
        assert_eq!(id.as_str(), "idle");
    }

    #[test]
    fn state_id_equality() {
        assert_eq!(StateId::new("a"), StateId::new("a"));
        assert_ne!(StateId::new("a"), StateId::new("b"));
    }

    #[test]
    fn state_id_display() {
        assert_eq!(format!("{}", StateId::new("running")), "running");
    }

    #[test]
    fn state_id_from_str() {
        let id: StateId = "idle".into();
        assert_eq!(id, StateId::new("idle"));
    }

    // -- Event tests --

    #[test]
    fn event_new() {
        let e = Event::new("tick");
        assert_eq!(e.name, "tick");
        assert!(e.data.is_empty());
    }

    #[test]
    fn event_with_data() {
        let e = Event::new("move").with_data("speed", 3.5);
        assert_eq!(e.data.get("speed").copied(), Some(3.5));
    }

    // -- Transition tests --

    #[test]
    fn transition_matches_basic() {
        let t = Transition::new("idle", "run", "start");
        assert!(t.matches(&StateId::new("idle"), &Event::new("start")));
    }

    #[test]
    fn transition_no_match_wrong_state() {
        let t = Transition::new("idle", "run", "start");
        assert!(!t.matches(&StateId::new("run"), &Event::new("start")));
    }

    #[test]
    fn transition_no_match_wrong_event() {
        let t = Transition::new("idle", "run", "start");
        assert!(!t.matches(&StateId::new("idle"), &Event::new("stop")));
    }

    // -- State tests --

    #[test]
    fn state_new() {
        let s = State::new("idle");
        assert_eq!(s.id, StateId::new("idle"));
        assert!(s.is_root());
        assert!(!s.has_children());
    }

    #[test]
    fn state_with_parent() {
        let mut s = State::new("run");
        s.parent = Some(StateId::new("move"));
        assert!(!s.is_root());
    }

    // -- StateMachine basic tests --

    #[test]
    fn sm_new() {
        let sm = StateMachine::new();
        assert!(sm.current.is_none());
        assert!(sm.history.is_empty());
        assert_eq!(sm.tick, 0);
    }

    #[test]
    fn sm_add_state_and_start() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        assert!(sm.start(StateId::new("idle")).is_ok());
        assert_eq!(sm.current_id(), Some(StateId::new("idle")));
    }

    #[test]
    fn sm_start_missing_state() {
        let mut sm = StateMachine::new();
        let result = sm.start(StateId::new("ghost"));
        assert!(result.is_err());
    }

    #[test]
    fn sm_process_event_transition() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        sm.add_transition(Transition::new("idle", "run", "start"));
        sm.start(StateId::new("idle")).unwrap();

        let next = sm.process_event(Event::new("start"));
        assert_eq!(next, Some(StateId::new("run")));
        assert_eq!(sm.current_id(), Some(StateId::new("run")));
    }

    #[test]
    fn sm_process_event_no_match() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.start(StateId::new("idle")).unwrap();

        let next = sm.process_event(Event::new("nothing"));
        assert!(next.is_none());
    }

    #[test]
    fn sm_is_in_state_current() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.start(StateId::new("idle")).unwrap();
        assert!(sm.is_in_state(&StateId::new("idle")));
    }

    #[test]
    fn sm_is_in_state_parent() {
        let mut sm = StateMachine::new();
        let mut parent = State::new("move");
        parent.children.push(StateId::new("run"));
        let mut child = State::new("run");
        child.parent = Some(StateId::new("move"));
        sm.add_state(parent);
        sm.add_state(child);
        sm.start(StateId::new("run")).unwrap();
        assert!(sm.is_in_state(&StateId::new("move")));
        assert!(sm.is_in_state(&StateId::new("run")));
    }

    #[test]
    fn sm_is_in_state_negative() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        sm.start(StateId::new("idle")).unwrap();
        assert!(!sm.is_in_state(&StateId::new("run")));
    }

    #[test]
    fn sm_history() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("a"));
        sm.add_state(State::new("b"));
        sm.add_transition(Transition::new("a", "b", "go"));
        sm.start(StateId::new("a")).unwrap();
        sm.process_event(Event::new("go"));
        assert_eq!(sm.history(), &[StateId::new("a"), StateId::new("b")]);
    }

    #[test]
    fn sm_tick_increments() {
        let mut sm = StateMachine::new();
        sm.tick();
        sm.tick();
        assert_eq!(sm.tick_count(), 2);
    }

    #[test]
    fn sm_reset() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.start(StateId::new("idle")).unwrap();
        sm.tick();
        sm.reset();
        assert!(sm.current.is_none());
        assert!(sm.history.is_empty());
        assert_eq!(sm.tick_count(), 0);
    }

    #[test]
    fn sm_available_transitions() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        sm.add_state(State::new("walk"));
        sm.add_transition(Transition::new("idle", "run", "start"));
        sm.add_transition(Transition::new("idle", "walk", "stroll"));
        sm.start(StateId::new("idle")).unwrap();

        let avail = sm.available_transitions();
        assert_eq!(avail.len(), 2);
    }

    #[test]
    fn sm_available_transitions_none() {
        let sm = StateMachine::new();
        assert!(sm.available_transitions().is_empty());
    }

    #[test]
    fn sm_priority_highest_wins() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        sm.add_state(State::new("walk"));
        let mut t1 = Transition::new("idle", "run", "go");
        t1.priority = 1;
        let mut t2 = Transition::new("idle", "walk", "go");
        t2.priority = 5;
        sm.add_transition(t1);
        sm.add_transition(t2);
        sm.start(StateId::new("idle")).unwrap();

        let next = sm.process_event(Event::new("go"));
        assert_eq!(next, Some(StateId::new("walk"))); // higher priority
    }

    #[test]
    fn sm_guard_blocks_in_process_event() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        let mut t = Transition::new("idle", "run", "go");
        t.guard = Some("always_false".to_string());
        sm.add_transition(t);
        sm.start(StateId::new("idle")).unwrap();

        // process_event skips guarded transitions
        let next = sm.process_event(Event::new("go"));
        assert!(next.is_none());
    }

    #[test]
    fn sm_guard_with_evaluator() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        let mut t = Transition::new("idle", "run", "go");
        t.guard = Some("check_speed".to_string());
        sm.add_transition(t);
        sm.start(StateId::new("idle")).unwrap();

        let mut guards = GuardEvaluator::new();
        guards.register("check_speed", |e| {
            e.data.get("speed").copied().unwrap_or(0.0) > 5.0
        });

        // Guard fails
        let next = sm.process_event_with_guards(Event::new("go"), &guards);
        assert!(next.is_none());

        // Guard passes
        let next = sm.process_event_with_guards(
            Event::new("go").with_data("speed", 10.0),
            &guards,
        );
        assert_eq!(next, Some(StateId::new("run")));
    }

    #[test]
    fn sm_hierarchical_transition() {
        let mut sm = StateMachine::new();
        let mut parent = State::new("move");
        parent.children.push(StateId::new("run"));
        let mut child = State::new("run");
        child.parent = Some(StateId::new("move"));
        sm.add_state(parent);
        sm.add_state(State::new("idle"));
        sm.add_state(child);
        // Transition from parent state "move"
        sm.add_transition(Transition::new("move", "idle", "stop"));
        sm.start(StateId::new("run")).unwrap();

        // Should find transition from parent "move" while in child "run"
        let next = sm.process_event(Event::new("stop"));
        assert_eq!(next, Some(StateId::new("idle")));
    }

    // -- Builder tests --

    #[test]
    fn builder_basic() {
        let sm = StateMachineBuilder::new()
            .state("idle")
            .on_enter("enter_idle")
            .on_exit("exit_idle")
            .state("run")
            .on_enter("enter_run")
            .transition("idle", "run", "start")
            .build();

        assert!(sm.states.contains_key(&StateId::new("idle")));
        assert!(sm.states.contains_key(&StateId::new("run")));
        assert_eq!(sm.transitions.len(), 1);
        assert_eq!(
            sm.states.get(&StateId::new("idle")).unwrap().on_enter,
            Some("enter_idle".to_string())
        );
    }

    #[test]
    fn builder_with_parent() {
        let sm = StateMachineBuilder::new()
            .state("move")
            .state("run")
            .parent("move")
            .build();

        let move_state = sm.states.get(&StateId::new("move")).unwrap();
        assert!(move_state.children.contains(&StateId::new("run")));
        let run_state = sm.states.get(&StateId::new("run")).unwrap();
        assert_eq!(run_state.parent, Some(StateId::new("move")));
    }

    #[test]
    fn builder_transition_with_guard_and_priority() {
        let sm = StateMachineBuilder::new()
            .state("idle")
            .state("run")
            .transition("idle", "run", "go")
            .guard("check")
            .priority(42)
            .build();

        let t = &sm.transitions[0];
        assert_eq!(t.guard, Some("check".to_string()));
        assert_eq!(t.priority, 42);
    }

    // -- GuardEvaluator tests --

    #[test]
    fn guard_evaluator_register_and_evaluate() {
        let mut ge = GuardEvaluator::new();
        ge.register("positive", |e| {
            e.data.get("val").copied().unwrap_or(0.0) > 0.0
        });
        let result = ge.evaluate("positive", &Event::new("x").with_data("val", 5.0));
        assert!(result);
        let result = ge.evaluate("positive", &Event::new("x").with_data("val", -1.0));
        assert!(!result);
    }

    #[test]
    fn guard_evaluator_unknown_passes() {
        let ge = GuardEvaluator::new();
        assert!(ge.evaluate("nonexistent", &Event::new("x")));
    }

    // -- AgentStateMachine tests --

    #[test]
    fn agent_sm_has_all_states() {
        let sm = AgentStateMachine::new();
        for name in &[
            "Idle", "Wander", "FollowPlayer", "Flee", "Interact", "Work", "Rest", "Alert",
        ] {
            assert!(sm.states.contains_key(&StateId::new(*name)), "missing state {}", name);
        }
    }

    #[test]
    fn agent_sm_idle_to_wander() {
        let mut sm = AgentStateMachine::new();
        sm.start(StateId::new("Idle")).unwrap();
        let next = sm.process_event(Event::new("timer"));
        assert_eq!(next, Some(StateId::new("Wander")));
    }

    #[test]
    fn agent_sm_danger_to_flee() {
        let mut sm = AgentStateMachine::new();
        sm.start(StateId::new("Wander")).unwrap();
        let next = sm.process_event(Event::new("danger"));
        assert_eq!(next, Some(StateId::new("Flee")));
    }

    #[test]
    fn agent_sm_conservation_error_to_alert() {
        let mut sm = AgentStateMachine::new();
        sm.start(StateId::new("Work")).unwrap();
        let next = sm.process_event(Event::new("conservation_error"));
        assert_eq!(next, Some(StateId::new("Alert")));
    }

    // -- Serde tests --

    #[test]
    fn serde_roundtrip_state_id() {
        let id = StateId::new("idle");
        let json = serde_json::to_string(&id).unwrap();
        let back: StateId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn serde_roundtrip_event() {
        let e = Event::new("tick").with_data("x", 1.0);
        let json = serde_json::to_string(&e).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "tick");
        assert_eq!(back.data.get("x").copied(), Some(1.0));
    }

    #[test]
    fn serde_roundtrip_state() {
        let s = State::new("idle");
        let json = serde_json::to_string(&s).unwrap();
        let back: State = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, StateId::new("idle"));
    }

    #[test]
    fn serde_roundtrip_transition() {
        let mut t = Transition::new("a", "b", "go");
        t.guard = Some("check".to_string());
        t.priority = 7;
        let json = serde_json::to_string(&t).unwrap();
        let back: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.from, StateId::new("a"));
        assert_eq!(back.to, StateId::new("b"));
        assert_eq!(back.guard, Some("check".to_string()));
        assert_eq!(back.priority, 7);
    }

    #[test]
    fn serde_roundtrip_state_machine() {
        let mut sm = StateMachine::new();
        sm.add_state(State::new("idle"));
        sm.add_state(State::new("run"));
        sm.add_transition(Transition::new("idle", "run", "start"));
        sm.start(StateId::new("idle")).unwrap();

        let json = serde_json::to_string(&sm).unwrap();
        let back: StateMachine = serde_json::from_str(&json).unwrap();
        assert_eq!(back.states.len(), 2);
        assert_eq!(back.transitions.len(), 1);
        assert_eq!(back.current, Some(StateId::new("idle")));
    }
}
