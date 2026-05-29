# Numberphile: Chess Knight Problems on a Numbered Spiral Grid

Based on the Numberphile video featuring Neil Sloane, here are the precise definitions of three distinct mathematical problems centered around the behavior of chess knights on a numbered square spiral grid. These definitions are structured to be easily interpreted by a coding agent for creating visualizations.

## The Environment: The Numbered Spiral Grid
All three problems take place on the same foundational board:
* **The Grid:** An infinite 2D square lattice (like a standard chessboard, but extending infinitely in all directions).
* **The Numbering System:** The squares are numbered sequentially with integers (0, 1, 2, 3, ...) moving outward from the center in a continuous **square spiral**.
* **Movement:** All distance and attack checks rely on the standard movement of a chess knight (an "L-shape": 2 squares horizontally and 1 vertically, or 2 vertically and 1 horizontally).

---

## Problem 1: The Trapped Knight Sequence
This is a sequence generation problem based on a single knight navigating the board.

* **Initialization:** Place a single knight on square `0`. Mark square `0` as "visited".
* **The Rule:** On each turn, the knight must move to the **lowest-numbered unvisited square** that is exactly one valid knight's move away from its current position.
* **The Outcome:** The sequence eventually terminates. The visualization should end when the knight gets "trapped" on a square where all 8 possible outward knight moves land on squares it has already visited.
* **Validated outcome:** From the center, the knight makes **2015 moves**, visiting **2016 squares** in total (the start plus 2015 hops), before it is trapped. With the center numbered `0` (as defined above) the trap is square **2083**; with the center numbered `1` — the Numberphile / OEIS [A316667](https://oeis.org/A316667) convention — it is the well-known square **2084**.

---

## Problem 2: Courteous Knights (Single-Color Placement)
This is a cellular automaton-style problem where the grid is populated iteratively without moving the pieces once placed.

* **Initialization:** Start an iteration loop from $n = 0$ moving sequentially upwards ($n = 0, 1, 2, 3, \dots$). Place a knight on square `0`.
* **The Rule:** For each square $n$ along the spiral, check if it is "attacked" by any knight that has *already* been placed on the board. 
    * If **YES** (a previously placed knight could reach square $n$ in one move): Leave the square empty.
    * If **NO** (the square is safe from all placed knights): Permanently place a new knight on square $n$.
* **The Outcome:** This generates a predictable, mathematically periodic pattern (OEIS [A308885](https://oeis.org/A308885)).
* **Validated outcome:** The knights settle into a regular crystal of three repeating motifs — **plus-pentominoes (5 knights)**, **2×2 squares (4 knights)**, and **lone knights (1)** — with occasional pairs (2) and triples (3) appearing only along the seams between phase-shifted domains. (An earlier description of "clusters of 2, 4, and 5" was a misremembering; the dominant cluster sizes are 5, 4, and 1.)

---

## Problem 3: Red & Black Knights (Two-Color Competition)
This is a two-player turn-based placement game that results in massive, unpredictable fractal-like patterns.

* **Initialization:** Two teams: Black and Red. The board is completely empty. Players take alternating turns, starting with Black.
* **Black's Turn Rule:** Scan the spiral starting from square `0`. Place a Black knight on the **lowest-numbered unoccupied square** that is **NOT** currently being attacked by any existing **Red** knight. *(Note: It is perfectly fine if the square is attacked by another Black knight; pieces of the same color cooperate/ignore each other).*
* **Red's Turn Rule:** Scan the spiral starting from square `0`. Place a Red knight on the **lowest-numbered unoccupied square** that is **NOT** currently being attacked by any existing **Black** knight. *(Again, it can be attacked by friendly Red knights).*
* **The Outcome:** Initially, the board looks like random noise with strange islands of empty squares. However, as the grid expands to millions of squares, the chaos aggressively sorts itself into massive, solid quadrants of territory controlled exclusively by one color.
* **Validated outcome:** OEIS [A392177](https://oeis.org/A392177) (Black squares) and [A392178](https://oeis.org/A392178) (Red squares), with the center numbered `0` — Black opens on square `0`, Red on square `1`. (Verified against the first 10000 terms of each b-file.) Opposite colors are never a knight's move apart, but same-color knights may be (they cooperate). The solid single-color territories, separated by woven checkerboard seams, emerge early — already clearly visible by radius ~80.

---
**Reference Video:** [Red & Black Knights (extraordinary result) - Numberphile](https://www.youtube.com/watch?v=UiX4CFIiegM)
