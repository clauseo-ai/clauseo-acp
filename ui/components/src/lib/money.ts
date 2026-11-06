// Money and counts as DESIGN writes them (design/DESIGN.md, the money rule): the rupee sign, Indian grouping and
// Western digits, the same in either language. The host sends money as whole paise; the app writes it here, so no
// figure is ever words, and never "Rs.". A whole number of rupees has no decimals (₹236); anything else has two
// (₹21.24). Credits are a count in the same grouping, without a sign ("10,00,000").

const WHOLE = new Intl.NumberFormat("en-IN-u-nu-latn", {
  style: "currency",
  currency: "INR",
  minimumFractionDigits: 0,
  maximumFractionDigits: 0,
});

const PAISE = new Intl.NumberFormat("en-IN-u-nu-latn", {
  style: "currency",
  currency: "INR",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

const COUNT = new Intl.NumberFormat("en-IN-u-nu-latn", { maximumFractionDigits: 0 });

/** Whole paise as rupees: 23_600 is "₹236", 2_124 is "₹21.24", 1_18_00_000 is "₹1,18,000". */
export function rupees(paise: number): string {
  const whole = Math.round(paise);
  return whole % 100 === 0 ? WHOLE.format(whole / 100) : PAISE.format(whole / 100);
}

/** A count in Indian grouping, no sign: 1_000_000 is "10,00,000". */
export const count = (n: number): string => COUNT.format(n);
