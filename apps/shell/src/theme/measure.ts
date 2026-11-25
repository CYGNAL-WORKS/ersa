// Lengths read from the design tokens on the page, so components that measure keep the tokens as the one source
// (docs/design/DESIGN.md).

/**
 * A length token as the page resolves it where `el` is drawn: pixels, or a share of one for a percentage. NaN when it is
 * unset or can't be read, as where no stylesheet is loaded; callers say what stands in for it.
 */
export function tokenLength(el: Element, name: string): number {
  const value = getComputedStyle(el).getPropertyValue(name).trim();
  return value.endsWith("%") ? Number.parseFloat(value) / 100 : Number.parseFloat(value);
}
