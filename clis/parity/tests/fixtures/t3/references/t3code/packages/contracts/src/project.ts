export const ProjectMutation = Schema.Union([
  Schema.Struct({ type: Schema.Literal("project.reached-create"), projectId: ProjectId }),
  Schema.Struct({ type: Schema.Literal("project.unreached-delete"), projectId: ProjectId }),
]);
