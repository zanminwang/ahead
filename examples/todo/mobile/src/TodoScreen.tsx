import { useState } from "react";
import {
  FlatList,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import type { Todo } from "../../generated/mobile/client";
import type { TodoState } from "./useTodos";

const avatarColors: Record<string, string> = { alice: "#3B6FD9", bob: "#2E9E6B" };

export function TodoScreen({ state }: { state: TodoState }) {
  const [draft, setDraft] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  async function submit() {
    const title = draft.trim();
    if (!title || submitting) return;
    setSubmitting(true);
    try {
      await state.add(title);
      setDraft("");
      setLocalError(null);
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : "Could not add task");
    } finally {
      setSubmitting(false);
    }
  }

  async function toggle(todo: Todo) {
    try {
      await state.setDone(todo.id, !todo.done);
      setLocalError(null);
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : "Could not update task");
    }
  }

  const name = state.user?.name ?? "";
  const message = localError ?? state.error;
  return (
    <KeyboardAvoidingView
      style={styles.screen}
      behavior={Platform.OS === "ios" ? "padding" : undefined}
    >
      <View style={styles.header}>
        <View style={[styles.avatar, { backgroundColor: avatarColors[state.user?.id ?? ""] ?? "#888" }]}>
          <Text style={styles.avatarText}>{name.slice(0, 1).toUpperCase()}</Text>
        </View>
        <Text style={styles.name}>{name}</Text>
      </View>
      <Text style={styles.heading}>To-do</Text>
      {state.phase === "loading" ? (
        <Text style={styles.note}>Loading the shared list…</Text>
      ) : state.phase === "needs-connection" ? (
        <Text style={styles.note}>Connect to the backend once to load the shared list.</Text>
      ) : null}
      <FlatList
        style={styles.list}
        data={state.todos}
        keyExtractor={(todo) => todo.id}
        keyboardShouldPersistTaps="handled"
        ItemSeparatorComponent={() => <View style={styles.separator} />}
        renderItem={({ item }) => (
          <Pressable
            accessibilityRole="checkbox"
            accessibilityState={{ checked: item.done }}
            accessibilityLabel={item.title}
            onPress={() => void toggle(item)}
            style={styles.row}
          >
            <View style={[styles.box, item.done && styles.boxDone]}>
              {item.done ? <Text style={styles.check}>✓</Text> : null}
            </View>
            <Text style={[styles.title, item.done && styles.titleDone]}>{item.title}</Text>
          </Pressable>
        )}
      />
      {message ? <Text style={styles.error}>{message}</Text> : null}
      <View style={styles.composer}>
        <TextInput
          style={styles.input}
          value={draft}
          onChangeText={setDraft}
          placeholder="Add task…"
          placeholderTextColor="#9A9A9A"
          returnKeyType="done"
          submitBehavior="submit"
          onSubmitEditing={() => void submit()}
          editable={state.phase === "ready"}
          accessibilityLabel="Add task"
        />
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Add"
          disabled={!draft.trim() || submitting || state.phase !== "ready"}
          onPress={() => void submit()}
          style={({ pressed }) => [styles.plus, pressed && styles.plusPressed]}
        >
          <Text style={styles.plusText}>+</Text>
        </Pressable>
      </View>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: "#FAFAFA" },
  header: { flexDirection: "row", alignItems: "center", paddingHorizontal: 20, paddingTop: 8, paddingBottom: 12 },
  avatar: { width: 36, height: 36, borderRadius: 18, alignItems: "center", justifyContent: "center", marginRight: 10 },
  avatarText: { color: "#FFF", fontSize: 17, fontWeight: "600" },
  name: { fontSize: 17, fontWeight: "600", color: "#111" },
  heading: { fontSize: 28, fontWeight: "700", color: "#111", paddingHorizontal: 20, paddingBottom: 8 },
  note: { color: "#666", paddingHorizontal: 20, paddingBottom: 8 },
  list: { flex: 1 },
  separator: { height: StyleSheet.hairlineWidth, backgroundColor: "#DDD", marginLeft: 56 },
  row: { flexDirection: "row", alignItems: "center", minHeight: 48, paddingHorizontal: 20, paddingVertical: 10 },
  box: { width: 24, height: 24, borderRadius: 6, borderWidth: 1.5, borderColor: "#888", marginRight: 12, alignItems: "center", justifyContent: "center" },
  boxDone: { backgroundColor: "#3B6FD9", borderColor: "#3B6FD9" },
  check: { color: "#FFF", fontSize: 15, fontWeight: "700" },
  title: { flex: 1, fontSize: 17, color: "#111" },
  titleDone: { color: "#8A8A8A", textDecorationLine: "line-through" },
  error: { color: "#B3261E", paddingHorizontal: 20, paddingVertical: 6 },
  composer: { flexDirection: "row", alignItems: "center", paddingHorizontal: 16, paddingVertical: 10, borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: "#DDD", backgroundColor: "#FFF" },
  input: { flex: 1, fontSize: 17, minHeight: 44, paddingHorizontal: 12, color: "#111" },
  plus: { width: 44, height: 44, borderRadius: 22, alignItems: "center", justifyContent: "center", backgroundColor: "#3B6FD9" },
  plusPressed: { opacity: 0.7 },
  plusText: { color: "#FFF", fontSize: 26, lineHeight: 28, fontWeight: "500" },
});
