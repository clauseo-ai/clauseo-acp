// A matter's name from the folder picked (folderName): the folder's own name, cleaned of what a name may not hold.
import { expect, test } from "vitest";
import { folderName } from "./folder-path.ts";

test("the folder's own name, whatever the path is written with", () => {
  expect(folderName("Documents/Health Insurance")).toBe("Health Insurance");
  expect(folderName("Documents\\Clients\\Rao v. Union")).toBe("Rao v. Union");
  expect(folderName("/Volumes/Work/Acme/")).toBe("Acme");
  expect(folderName("D:\\Cases")).toBe("Cases");
});

test("controls and bidi marks are dropped, the name trimmed and cut to 200 characters", () => {
  expect(folderName("Documents/ a\u202Efdp\u0007 ")).toBe("afdp");
  expect(Array.from(folderName(`Documents/${"क".repeat(250)}`))).toHaveLength(200);
});
