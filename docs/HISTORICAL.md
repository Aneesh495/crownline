# Historical tournament holdout

The frozen WCDF holdout contains the 2007 Las Vegas GAYP and 2011 San Remo
GAYP archives. The manifest pins their exact bytes, source order, dialect,
record counts and the three preview model identities. The acquisition
command verifies every downloaded byte before saving it.
[WCDF game downloads](https://wcdf.net/games.htm)

Run the native evaluation against the extracted preview evidence bundle.

    make historical HISTORICAL_MODELS=/path/to/crownline-evidence

The command attempts all 174 source records. It saves an observation for
each, including raw byte offsets, hash, turn count, declared result, final
board and any parse diagnostic. The source files stay in the configured
acquisition directory. The output contains the derived observations and
can be replayed by fetching the pinned inputs again.

The original training stream and model files must match their frozen
hashes. The evaluator excludes every canonical board visited during that
stream, then removes duplicate historical sample boards. It samples
before plies 16, 32, 48 and 64 and obtains a 4,096-node classical teacher
score. Labels from historical reported results do not enter training.

The separate verifier reconstructs the training exclusion set with the
coordinate-array generator. It checks every historical transition,
sample position, teacher search, exported network output and aggregate
error against the saved observations. A changed teacher target, model
identity or RMSE makes verification fail.
[Evaluation](../src/lab/historical.rs) · [Independent verification](../src/lab/verify_historical.rs)

The paired strength requirement keeps its frozen arena result. Historical
RMSE measures agreement with the stated teacher at sampled positions.
