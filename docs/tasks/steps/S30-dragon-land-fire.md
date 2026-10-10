# S30: The dragon lands (PressAttack) and breathes fire (BreathWide)

- **Branch:** `step/s30-dragon-land-fire` (after S29)
- **Files:** `crates/ddenemyrt/src/dragon*.rs`, host/view hooks, tests
- **Kind:** one feature

## Context (S24 map)

State 0 PressAttack (33): motion 298 then 299, `0x00A6AED0/0x00A6AF70`,
falling/ground contact, parameter-driven acceleration from
`cCharParamEmDragon`, landing consequences; its 150-frame branch can
dispatch EscapeFly (60). States 7/8 wait motion frame >= 10, then motion
end. State 2 BreathWide (21): 263 then 264, `0x00A56A30/0x00A56D10`, exit
`0x00A57270`, frame-gated breath collisions, emitter/joint targeting,
damage. State 2 sets stage LOT 80 and q0 bit 20; state 9 timer 0 >= 5.3 s;
state 10 q0 bit 21; state 6 q0 bit 5 and Kill.

## Do

Translate PressAttack and BreathWide (with their breath volumes and
damage against the party), continue the FSM to state 6, and apply the
FSM's quest/LOT/GSF requests through the host.

## Done when

A headless test plays the entrance to state 6 with real feedback, the
breath damages an Arisen in its volume, the FSM's flags reach the story,
and a capture shows the landing and the fire.
