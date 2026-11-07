// ESLint flat config for services/agent/ and services/web/ (ADR 0049 §5, limits from §7).
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import boundaries from "eslint-plugin-boundaries";
import sonarjs from "eslint-plugin-sonarjs";
import { defineConfig } from "eslint/config";
import tseslint from "typescript-eslint";

export default defineConfig(
  { ignores: ["**/dist/**", "**/coverage/**", "**/gen/**", "**/node_modules/**"] },
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  {
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
  },
  {
    files: ["**/*.{ts,tsx}"],
    plugins: { boundaries, sonarjs },
    settings: {
      "import/resolver": {
        typescript: { project: ["services/agent/tsconfig.json", "services/web/tsconfig.json"] },
      },
      "boundaries/elements": [
        { type: "gen", pattern: "**/gen/**" },
        { type: "agent", pattern: "services/agent/src/**" },
        { type: "web", pattern: "services/web/src/**" },
      ],
    },
    rules: {
      // services/agent/ and services/web/ never import each other; both may import generated
      // contract types.
      "boundaries/dependencies": [
        "error",
        {
          default: "disallow",
          policies: [
            {
              from: { element: { type: "agent" } },
              allow: [{ to: { element: { type: "agent" } } }, { to: { element: { type: "gen" } } }],
            },
            {
              from: { element: { type: "web" } },
              allow: [{ to: { element: { type: "web" } } }, { to: { element: { type: "gen" } } }],
            },
          ],
        },
      ],
      "max-lines": ["error", { max: 400, skipBlankLines: false, skipComments: false }],
      "max-lines-per-function": ["error", { max: 60, skipBlankLines: false, skipComments: false }],
      "max-params": ["error", 5],
      "sonarjs/cognitive-complexity": ["error", 15],
      "no-console": "error",
      // No `as` casts: external data is parsed with zod at the edge (ADR 0049 §5).
      "@typescript-eslint/consistent-type-assertions": ["error", { assertionStyle: "never" }],
    },
  },
  // The agent's event writer is its one console sink.
  { files: ["services/agent/src/events/events.ts"], rules: { "no-console": "off" } },
  // Test files don't count towards file length (ADR 0049 §7).
  { files: ["**/*.test.{ts,tsx}", "**/tests/**"], rules: { "max-lines": "off" } },
  { files: ["**/*.js"], extends: [tseslint.configs.disableTypeChecked] },
  prettier,
);
