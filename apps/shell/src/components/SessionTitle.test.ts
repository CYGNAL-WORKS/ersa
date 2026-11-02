import { describe, expect, it } from "vitest";
import { renameTo, titleToSave } from "./SessionTitle";

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
  });

  it("leaves spacing inside the title to Core", () => {
    expect(titleToSave(" Fix  the bug ", "Old")).toBe("Fix  the bug");
  });

});

describe("renameTo, what Save sends", () => {
  it("sends the placeholder's words typed on an untitled session", () => {
    expect(renameTo("Untitled session", "Untitled session", true)).toBe("Untitled session");
  });

  it("sends nothing for a titled session's own title", () => {
    expect(renameTo("Untitled session", "Untitled session", false)).toBeUndefined();
  });
});
