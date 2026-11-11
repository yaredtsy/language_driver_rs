"""Step 01.04: `import pkg` alone does not make `pkg.sub` an attribute.
pkg/__init__.py imports nothing, so Python raises AttributeError here, and
the walker must not find an edge either."""

import pkg


def no_submodule():
    pkg.sub.mod.work()


# expect:
# no_submodule
