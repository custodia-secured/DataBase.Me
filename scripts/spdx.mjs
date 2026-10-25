// SPDX-License-Identifier: AGPL-3.0-or-later
// A small SPDX licence-expression evaluator, enough for the licence fields found on npm.
//
// `OR` is satisfied when either side is allowed, `AND` only when both are, and `X WITH exception`
// counts as `X` (licence exceptions only grant extra permissions). Anything that is not a valid
// expression ("SEE LICENSE IN ...", "UNKNOWN", an empty field) is rejected.

/**
 * @param {string} expression SPDX licence expression, e.g. "(MIT OR Apache-2.0) AND ISC".
 * @param {(id: string) => boolean} isAllowed decides a single licence identifier.
 * @returns {boolean}
 */
export function expressionAllowed(expression, isAllowed) {
  const tokens = tokenize(expression);
  if (tokens === null || tokens.length === 0) return false;
  let pos = 0;

  /** @returns {boolean | null} null when the expression is malformed */
  function parseOr() {
    let value = parseAnd();
    if (value === null) return null;
    while (tokens[pos] === "OR") {
      pos += 1;
      const rhs = parseAnd();
      if (rhs === null) return null;
      value = value || rhs;
    }
    return value;
  }

  function parseAnd() {
    let value = parseAtom();
    if (value === null) return null;
    while (tokens[pos] === "AND") {
      pos += 1;
      const rhs = parseAtom();
      if (rhs === null) return null;
      value = value && rhs;
    }
    return value;
  }

  function parseAtom() {
    const token = tokens[pos];
    if (token === undefined) return null;
    if (token === "(") {
      pos += 1;
      const value = parseOr();
      if (tokens[pos] !== ")") return null;
      pos += 1;
      return value;
    }
    if (token === ")" || token === "AND" || token === "OR" || token === "WITH") return null;
    pos += 1;
    if (tokens[pos] === "WITH") {
      pos += 2;
      if (tokens[pos - 1] === undefined) return null;
    }
    return isAllowed(token.replace(/\+$/, ""));
  }

  const result = parseOr();
  return result !== null && pos === tokens.length && result;
}

/** @returns {string[] | null} */
function tokenize(expression) {
  if (typeof expression !== "string") return null;
  const tokens = expression.replace(/[()]/g, " $& ").trim().split(/\s+/).filter(Boolean);
  const idPattern = /^[A-Za-z0-9.+-]+$/;
  for (const token of tokens) {
    if (token === "(" || token === ")") continue;
    if (!idPattern.test(token)) return null;
  }
  return tokens.map((t) => (/^(and|or|with)$/i.test(t) ? t.toUpperCase() : t));
}
