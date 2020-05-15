# Learned evaluator card

The independent network uses 128 sparse piece-square features in mover
coordinates, 32 context bins, one 24-lane clipped ReLU layer and a shared
head evaluated for both colors. The integer output subtracts the two
perspectives. Inputs and bias use scale 256, the head scale 512. Activations
are clipped before the dot product and the result uses a wide intermediate
for conversion to score units. Trained linear weights use the classical
feature definitions with separate fitted coefficients.

The fitting input contains original 256-node self-play games. Labels mix
the deeper 2,048-node teacher and game outcomes in mover convention.
Groups are assigned before fitting. Canonical boards spanning multiple
splits are excluded. No external game collection contributed to these
models. The models estimate the chosen teacher target and are not ratings
or WDL probabilities.

The [experiment plan](EXPERIMENT_PLAN.md) gives budgets, seeds and
promotion criteria. The [trainer](../src/lab/train.rs) freezes probes on
validation examples. Incremental inference must match the independent
scan and the disabled-weight control must differ on at least one probe.
The product defaults to the untuned classical evaluator.
