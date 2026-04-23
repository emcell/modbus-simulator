import { initGraphQLTada } from "gql.tada";
import { print, type DocumentNode } from "graphql";
import type { introspection } from "./graphql-env.d.ts";

export const graphql = initGraphQLTada<{
  introspection: introspection;
  scalars: {
    ID: string;
  };
}>();

const ENDPOINT = "/graphql";

export interface GqlError {
  message: string;
}

export class GraphQLRequestError extends Error {
  constructor(public errors: GqlError[]) {
    super(errors.map((e) => e.message).join("; "));
  }
}

type TadaDoc<TResult, TVariables> = {
  __apiType?: (variables: TVariables) => TResult;
};

export async function request<TResult, TVariables>(
  doc: TadaDoc<TResult, TVariables>,
  variables?: TVariables,
): Promise<TResult> {
  const query = print(doc as unknown as DocumentNode);
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ query, variables: variables ?? {} }),
  });
  const body = (await res.json()) as { data?: TResult; errors?: GqlError[] };
  if (body.errors && body.errors.length > 0) {
    throw new GraphQLRequestError(body.errors);
  }
  return body.data as TResult;
}
