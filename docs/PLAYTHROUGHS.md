# Complete-game coverage

The server tests run 44 complete sessions through the real HTTP and native
process interface. Eight replay a signed 42-turn fixture as local players
or an authenticated invite room under each of the four supported clock
settings. The other 36 combine three engine levels, both human-seat
assignments or engine-versus-engine play, and the four clock settings.

Scripted human-seat actions choose a legal route through the same command
endpoint used by the browser. The tests require an ordinary game result
and reimport every exported record. Reconnection and duplicate submission
checks occur inside a complete match. Competitive room participants can
export the finished game from the PDN panel.
[Test matrix](../web/tests/full-games.test.mjs)

Set the environment variable below to retain every completed game as JSONL.

    CROWNLINE_PLAYTHROUGH_LEDGER=/path/to/new.jsonl make test
    target/release/crownline-lab verify-playthroughs /path/to/new.jsonl

The writer requires a new file. Each row includes the scenario, settings,
full PDN, turn count and outcome. The independent checker requires every
planned scenario once, replays the entire turn sequence with the array
oracle and checks the final result. Session credentials are absent from
these records.
[Independent replay](../src/lab/verify.rs)

Chromium also executes the 42-turn fixture by selecting every square of
each full turn on the board. It checks the final status and saves a
screenshot. That path follows the existing keyboard, mobile, reload,
branch-review and cancellation checks.
[Browser walkthrough](../web/tests/browser.mjs)
