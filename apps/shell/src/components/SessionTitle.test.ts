import { describe, expect, it } from "vitest";
import { titleToSave } from "./SessionTitle";

describe("titleToSave", () => {
  it("sends the new title without the space around it", () => {
    expect(titleToSave("  Renamed  ", "Old")).toBe("Renamed");
  });

  it("sends nothing for a blank title, which no backend keeps", () => {
    expect(titleToSave("", "Old")).toBeUndefined();
    expect(titleToSave(" \n ", "Old")).toBeUndefined();
  });

  it("sends nothing when the title is unchanged", () => {
    expect(titleToSave("Old ", "Old")).toBeUndefined();
    expect(titleToSave("Old  title", "Old title")).toBeUndefined();
  });

  it("spaces the title as Core takes it", () => {
    expect(titleToSave("Fix  the\tbug\non Windows", "Old")).toBe("Fix the bug on Windows");
  });
});
