/**
 * Runtime validation of WorkflowDocument against the committed JSON Schema.
 * Uses ajv directly against the schema artifact — zero drift from the source.
 *
 * DO NOT EDIT — regenerate via codegen/generate.sh
 */
import Ajv from "ajv";
import { readFileSync, existsSync } from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));

function findSchemaPath(): string {
  const candidates = [
    // From src/generated/, walk up to newton root then to packages/workflow-schema/
    join(__dirname, "..", "..", "..", "..", "packages", "workflow-schema", "workflow.schema.json"),
    join(__dirname, "..", "..", "..", "..", "..", "packages", "workflow-schema", "workflow.schema.json"),
    // Absolute fallback for this workspace
    "/home/sysuser/ws001/gonewton/newton/packages/workflow-schema/workflow.schema.json",
  ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  throw new Error(
    `workflow.schema.json not found. Tried:\n${candidates.join("\n")}`
  );
}

// Load the committed schema artifact
let _schema: Record<string, unknown> | null = null;

function getSchema(): Record<string, unknown> {
  if (_schema === null) {
    const schemaPath = findSchemaPath();
    _schema = JSON.parse(readFileSync(schemaPath, "utf-8"));
  }
  return _schema!;
}

let _validate: ReturnType<Ajv["compile"]> | null = null;

function getValidator(): ReturnType<Ajv["compile"]> {
  if (_validate === null) {
    const schema = getSchema();
    // Strip $schema if present to avoid dialect-specific validation issues
    const schemaWithoutMeta = { ...schema };
    delete (schemaWithoutMeta as Record<string, unknown>)["$schema"];
    const ajv = new Ajv({ strict: false, allErrors: true });
    _validate = ajv.compile(schemaWithoutMeta);
  }
  return _validate;
}

export interface ValidationResult {
  valid: boolean;
  errors: string[];
}

/**
 * Validate a WorkflowDocument against the committed JSON Schema.
 * Returns { valid: true, errors: [] } on success, or { valid: false, errors: [...] } on failure.
 */
export function validateWorkflowDocument(doc: unknown): ValidationResult {
  const validate = getValidator();
  const valid = validate(doc) as boolean;
  if (valid) {
    return { valid: true, errors: [] };
  }
  const errors = (validate.errors ?? []).map((e) => {
    return `${e.instancePath || "/"} ${e.message ?? "unknown error"}`;
  });
  return { valid: false, errors };
}
