# Build status

Crownline has a playable native-engine preview, authoritative invite
rooms, PDN study, a closed four-piece board-only proof and an original
training and arena laboratory. Full acceptance is incomplete.

[Published preview and raw evidence](https://github.com/Aneesh495/crownline/releases/tag/v0.1.0-preview)
· [Observed results and source identity](RESULTS.md)

| Requirement | Implementation and checks |
|---|---|
| G01 to G03 | [Rules matrix](RULES_MATRIX.md) and independent transitions, perft and clock/claim tests |
| G04 to G05 | [Search oracle tests](../tests/search.rs) and [adversarial cases](../src/lab/tactics.rs) |
| G06 to G08 | [Independent closed proof](../src/tablebase/verify.rs) and [open frontier replay](../src/tablebase/frontier.rs) |
| G09 to G10 | [Model and data verifier](../src/lab/verify.rs). External tournament training holdout remains open |
| G11 | [Matched arena](../src/lab/arena.rs). The designated learned improvement gate failed |
| G12 to G15 | [Live Chromium](../web/tests/browser.mjs), [room integration](../web/tests/session.test.mjs), [PDN tests](../tests/pdn.rs) and [walkthroughs](WALKTHROUGHS.md). Full human playthrough coverage remains open |
| G16 | [Signed campaign verifier](../tools/verify_campaign.py) rejected eight altered bundles. Both native archives passed extracted-package replay |

Separate limitations include the board-only proof's draw abstraction,
room persistence across service restart, and incomplete historical game
coverage. The preview uses the classical evaluator.
