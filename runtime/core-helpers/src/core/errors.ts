export type FailureMetadata = {
  failureClass: string;
  message: string;
  remediation?: string;
};

export class RuntimeFailure extends Error {
  readonly failureClass: string;
  readonly remediation?: string;

  constructor(metadata: FailureMetadata) {
    super(metadata.message);
    this.name = 'RuntimeFailure';
    this.failureClass = metadata.failureClass;
    this.remediation = metadata.remediation;
  }
}
