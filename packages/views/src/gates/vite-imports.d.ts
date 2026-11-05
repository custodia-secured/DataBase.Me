// SPDX-License-Identifier: AGPL-3.0-or-later
// What Vite resolves in the gates: this package has no dependency on Vite's own client types.
declare module "*.css";
declare module "*?url" {
  const url: string;
  export default url;
}
