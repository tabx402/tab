import { FixtureHolderAccess } from "./holder-access-fixture-policy";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "../src/styles.css";
import { AgentAccessKey } from "../src/components/AgentAccessKey";
import { request } from "../src/lib/api";
function Preview() {
  const [agent, setAgent] = useState("agent-a");
  return <main><section className="panel"><p>local access key QA</p><button onClick={() => setAgent(agent === "agent-a" ? "agent-b" : "agent-a")}>switch agent</button><p>selected: {agent}</p><AgentAccessKey key={agent} agentId={agent} api={request} /></section></main>;
}
createRoot(document.getElementById("root")!).render(<FixtureHolderAccess><Preview /></FixtureHolderAccess>);
