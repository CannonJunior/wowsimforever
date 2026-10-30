# Real single-target Warlock priority lists, as extracted

Source: `validation/2026-10-06-warlock-multi-target-census/bases/2-targets/{spec}-warlock.request.json`
in `sage3648/mythicsim-forever-engine` (a 2-target fixture, but the priority list itself is
the spec's general-purpose single-target rotation - the multi-target branches live in the
`condition` trees for abilities this project doesn't implement, like Bane of Havoc).

Spell IDs decoded against this project's own data pack / the engine's `spell_data.rs` ladders.
Lines this project's engine doesn't model (an unidentified on-use ability at spell 1259821,
potion timing, `autocastOtherCooldowns`) are struck through rather than omitted, so the
original list is still fully visible here.

## Destruction

1. ~~spell 1259821, if Bane of Agony + Corruption + Immolate all active and fight time >= 10s~~
2. ~~Potion, if fight time remaining >= 15s and mana% <= 75%~~
3. ~~(same potion line again at mana% <= 60%)~~
4. ~~autocastOtherCooldowns~~
5. Shadowburn (18871), if fight time remaining <= 1.5s
6. Searing Pain (17923), if fight time remaining <= 3.5s
7. Life Tap (11689), if mana% < 10%
8. Curse of the Elements (1311680), if not already active
9. Bane of Agony (11713), if its remaining duration <= its own cast time
10. Corruption (25311), same clip-avoidance rule
11. Immolate (25309), same clip-avoidance rule
12. Conflagrate (18932), if Immolate is active AND (its remaining duration < 4s, OR Shadow and
    Flame is talented and its Fire buff isn't already up)
13. Shadowburn (18871), unconditionally (i.e. whenever off cooldown)
14. Shadow Bolt (25307), filler

This engine's `destruction.rs` implements 5-14 (folding 5+13 into one "cast whenever ready"
Shadowburn line, and adding an Incinerate-instead-of-Shadow-Bolt filler branch for builds that
have it talented, which this particular fixture's build did not).

## Affliction

1-4. (same struck-through lines as Destruction)
5. Searing Pain (17923), if fight time remaining <= 3.5s
6. Life Tap (11689), if mana% < 10%
7. Curse of the Elements (1311680), if not already active
8. Amplify Curse (18288), unconditionally (i.e. whenever off its 180s cooldown)
9. Bane of Agony (11713), clip-avoidance
10. Corruption (25311), clip-avoidance
11. Siphon Life (18881), clip-avoidance
12. Immolate (25309), clip-avoidance (yes, this Affliction build also keeps Immolate up)
13. Shadow Bolt (25307), filler

## Demonology

1-4. (same struck-through lines)
5. Searing Pain (17923), if fight time remaining <= 3.5s
6. Life Tap (11689), if mana% < 10%
7. Curse of the Elements (1311680), if not already active
8. Bane of Agony (11713), clip-avoidance
9. Corruption (25311), clip-avoidance
10. Immolate (25309), clip-avoidance
11. Searing Pain (17923) again, if the Demonic Brand talent is known AND the brand's remaining
    duration <= 3.5s - i.e. Searing Pain is cast *rotationally* here, not just as an end-of-fight
    filler, to keep the demon's Demonic Brand stacks up.
12. Shadow Bolt (25307), filler

This engine's `demonology.rs` implements lines 8-10 and 12 directly, and line 11 as of the
2026-10-23 follow-up pass: a landed Searing Pain now sets Demonic Brand charges (2/4/6 per
point, 10s), consumed by the pet's own landed hits for an extra brand-damage tick - see
`demonology.rs`'s module docs for the real (but less independently verified than the rest of
this pack) damage numbers behind it.
