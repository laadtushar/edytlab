import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState, type ReactElement, type ReactNode } from "react";

/** A local-IPC cache owned by one mounted App, never shared across apps. */
export function createSessionQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        networkMode: "always",
        retry: false,
        staleTime: Infinity,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      },
    },
  });
}

export function SessionQueryProvider({
  children,
}: { children: ReactNode }): ReactElement {
  const [client] = useState(createSessionQueryClient);
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
