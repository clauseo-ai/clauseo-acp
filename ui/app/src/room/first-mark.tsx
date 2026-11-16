// The page's first mark, as index.html paints it before the script (its #first-mark style stays in the head): React's
// first commit replaces it, so the room puts the same mark back while its first screen is undecided (deciding.ts).

/** index.html's #first-mark, element for element. */
export function FirstMark() {
  return (
    <div id="first-mark" aria-hidden="true" data-testid="first-mark">
      <svg viewBox="7752 944 411 695" fill="currentColor">
        <path d="M 8162.05 1149.65 C 8162.05 1188.03 8151 1275.08 8100.24 1368.41 C 8035.48 1487.55 7941.48 1552.6 7819.19 1638.22 L 7760.24 1553.94 C 7842.72 1496.22 7903.1 1431.36 7934.24 1347.75 C 7828.43 1332.79 7753 1237.46 7762.91 1131.08 C 7772.81 1024.7 7864.62 944.89 7971.38 949.84 C 8078.05 954.79 8162.05 1042.79 8162.05 1149.65 Z" />
      </svg>
    </div>
  );
}
