# lado

lado is a desktop viewer for git diffs. This glossary covers the language of the **C4 architecture
diff diagram** — a view of how a change alters the repository's architecture. The diagram is
decoupled from lado's existing diff-viewing functionality; it visualises the same change at a
different level.

## Language

### The diagram

**Component**:
A unit of the architecture the diagram depicts, at C4's Component level.
_Avoid_: node — that is the renderer's drawing primitive, not the domain concept.

**Relation**:
A communication path between two Components. What the diagram exists to show, and not something
a text diff can reveal.
_Avoid_: edge, dependency, call.

### The run

**Review target**:
The pair of repository states a diagram compares, `base..head`. Always two commits.
_Avoid_: range, diff.

**Diagram run**:
One production of a diagram for one Review target. Identified by base commit, head commit, and
the hash of the prompt file.
_Avoid_: task, job — there is a single operation, so nothing needs a type.
