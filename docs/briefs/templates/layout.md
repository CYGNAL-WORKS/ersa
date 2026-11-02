---
id: LAY-EXAMPLE
type: layout
title: Noun phrase naming the screen or region
status: draft
references: []
adrs: []
---

# Title

## Purpose

What this screen or region is for, in one or two sentences.

## Regions

An ASCII sketch of the layout at the default size, with each region labeled. Then one line per region: its name (a glossary term), what it contains (components by ID), and how it sizes (fixed, fills, collapsible, resizable).

```
+--------+----------------------------+-----------+
|        |                            |           |
|  rail  |        transcript          | workbench |
|        |                            |           |
|        +----------------------------+           |
|        |         composer           |           |
+--------+----------------------------+-----------+
```

- **rail**: contains `CMP-...`. Fixed width, collapsible.

## Responsive behavior

What changes when the window is narrow, short, or very wide. Which regions collapse first.

## Keyboard and focus

The focus order and the shortcuts this layout owns.

## Motion

What animates when regions appear, resize, or collapse, by name of the motion token.

## Requirements

- **LAY-EXAMPLE-R01**: ...

## Out of scope

## Open questions

## Changes

- YYYY-MM-DD: created.
