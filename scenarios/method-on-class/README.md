
**Bend** (`bend/`, BD4 step 4) has no methods. `Counter.bump` is one name that happens to have a dot in it (`GUIDE.md`: "dots inside a name
are just characters"), and `Counter.bump(c)` is a call to it like any other. Bend gives the shared tree, and what the scenario pins for it is
that a dotted name is one name, not that there is a receiver.
