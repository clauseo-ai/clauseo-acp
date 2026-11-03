// Words that keep an icon with them when they wrap: the chevron and the pen held to the last word, the break to the
// first, the text itself unchanged; a long word with no spaces keeps only its last few letters with the icon, so it
// can still break.
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { Glued } from "./glued.tsx";
import { headOf, tailOf } from "./glued-words.ts";

test("the last word goes with the icon after, the first with the icon before; a long word keeps only a few letters", () => {
  expect(tailOf("Reading Lease.pdf.")).toEqual(["Reading ", "Lease.pdf."]);
  expect(tailOf("1 विफल रहा।")).toEqual(["1 विफल ", "रहा।"]);
  expect(tailOf("one")).toEqual(["", "one"]);
  expect(tailOf("Reading Written_statement_of_respondent_final.pdf.")).toEqual([
    "Reading Written_statement_of_respondent_fina",
    "l.pdf.",
  ]);
  expect(headOf("1 failed.")).toEqual(["1", " failed."]);
  const [start, rest] = headOf("Written_statement_of_respondent_final.pdf failed.");
  expect(start).toHaveLength(6);
  expect(start + rest).toBe("Written_statement_of_respondent_final.pdf failed.");
});

test("Glued keeps the text whole and puts each icon in a no-wrap span with its word", () => {
  const html = renderToStaticMarkup(
    <p>
      <Glued before={<i>b</i>} after={<i>c</i>}>
        2 failed.
      </Glued>
    </p>,
  );
  expect(html).toBe(
    '<p><span class="glued"><i>b</i>2</span> <span class="glued">failed.<i>c</i></span></p>',
  );
  // one word holds both icons in one span
  expect(
    renderToStaticMarkup(
      <Glued before={<i>b</i>} after={<i>c</i>}>
        Failed.
      </Glued>,
    ),
  ).toBe('<span class="glued"><i>b</i>Failed.<i>c</i></span>');
  expect(renderToStaticMarkup(<Glued after={<i>c</i>}>Reading Lease.pdf.</Glued>)).toBe(
    'Reading <span class="glued">Lease.pdf.<i>c</i></span>',
  );
});
