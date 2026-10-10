# S08: Find the original's movie skip input (research only)

- **Branch:** `step/s08-movie-skip` (docs only)
- **Files:** `docs/tasks/opening-movie.md` (Result section)
- **Kind:** research, one question

## Question

When the opening movie (st100ev00.wmv) plays, which input skips it in the
original, and is skipping allowed the first time?

## How

1. Use the Ghidra headless server on 127.0.0.1:8089. Search strings for the
   movie class (`list_strings?filter=Movie`, also `.wmv`) and find the
   class that plays it (its vtable and update function).
2. In its update, find the input check that ends the movie early (calls
   into the pad/keyboard manager, button masks). Note the button mask and
   any "already seen" flag it checks.
3. If the pad mask matches a named button in docs (search docs for the
   mask), name it.

## Done when

The opening-movie Result says: the function address, the button mask(s),
the button names if known, and whether a seen-flag gates it. If you can't
find it within one hour of tracing, write down where you looked and stop.
