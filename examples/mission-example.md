+++
slug = "example"

[[repos]]
path = "/path/to/target-repo"
target_branch = "main"
gates = ["true"]

[[lanes]]
id = "L1"
owns = ["src/example/**"]
forbidden = []
tests = ["true"]
brief = "Example lane that owns src/example/."
+++

# Objective

Example mission used for smoke tests and as a authoring template.

## AC

- AC-1: The example lane exists and its tests pass.

## INV

- INV-1: No file outside `src/example/` is modified.

## NG

- NG-1: No public API change.
