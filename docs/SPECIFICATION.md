# Rules and position contract

Crownline targets the WCDF 2012 English rules edition. The reference PDF has
SHA-256 `aa1d1235632046c05db7621437f16c33bc7b86b472ccaa039a7a41b897b180b7`.
It refers to the first side as Red. Crownline uses Black in text protocols
to match PDN GameType 21. Black starts and advances toward 29 through 32.
White starts on 21 through 32 and advances toward 1 through 4.
[WCDF rules](https://wcdf.net/rules/rules_of_checkers_english.pdf)

A position contains disjoint side occupancy, a king subset, and a side to
move. Square numbers are 1 through 32. A full turn is a quiet step or an
ordered route through every landing in a compulsory capture. Promotion
ends a man's capture turn. Victims remain blockers until the full turn
ends and may be jumped only once.

Position interchange uses numeric FEN with explicit turn and both side
lists, for example `B:B1-12:W21-32`. Output lists individual squares with
a K prefix for kings. Whole turns use `9-13` or `10x17x26`. A capture
abbreviation is accepted only when exactly one legal route matches.
[PDN GameType](https://wiegerw.github.io/pdn/gametype.html)
[PDN FEN](https://wiegerw.github.io/pdn/fen.html)

No move loses. A WCDF repetition claim identifies a legal prospective
move whose resulting position has appeared twice. The no-progress claim
requires 40 previous turns per player with neither a man's advance nor
any capture. These claims require replay history. A position by itself
starts a new history. A board-only solver treats infinite play as a draw
and does not encode tournament claims or clocks.
