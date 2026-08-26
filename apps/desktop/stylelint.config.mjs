/** @type {import("stylelint").Config} */
export default {
  files: ["src/**/*.css"],
  rules: {
    "color-no-invalid-hex": true,
    "function-calc-no-unspaced-operator": true,
    "no-invalid-double-slash-comments": true,
    "no-invalid-position-at-import-rule": true,
    "selector-anb-no-unmatchable": true,
  },
};
