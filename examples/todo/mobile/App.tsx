import { useEffect, useState } from "react";
import { SafeAreaView, StyleSheet, Text } from "react-native";
import { StatusBar } from "expo-status-bar";
import { loadConfig, type LaunchConfig } from "./src/config";
import { TodoScreen } from "./src/TodoScreen";
import { useTodos } from "./src/useTodos";
import { runSmoke } from "./src/smoke";

function Todo({ config }: { config: LaunchConfig }) {
  const state = useTodos(config);
  return <TodoScreen state={state} />;
}

/** Test builds run the diagnostic sequence from config.json; ordinary launches render the screen. */
function Smoke({ config }: { config: LaunchConfig }) {
  const [result, setResult] = useState("running diagnostics");
  useEffect(() => {
    void runSmoke(config, setResult);
  }, [config]);
  return <Text selectable style={styles.diagnostic}>{result}</Text>;
}

export default function App() {
  const [config, setConfig] = useState<LaunchConfig | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  useEffect(() => {
    loadConfig().then(setConfig, (error) => setFailure(String(error)));
  }, []);
  return (
    <SafeAreaView style={styles.root}>
      <StatusBar style="dark" />
      {failure ? (
        <Text style={styles.diagnostic}>{failure}</Text>
      ) : config === null ? null : config.phase ? (
        <Smoke config={config} />
      ) : (
        <Todo config={config} />
      )}
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, backgroundColor: "#FAFAFA" },
  diagnostic: { padding: 24, fontSize: 15, lineHeight: 22 },
});
