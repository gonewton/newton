/**
 * Shared compiler error type — imported by both refs.ts and checks.ts
 * to avoid a circular dependency.
 */
export class CompilerError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CompilerError";
  }
}
