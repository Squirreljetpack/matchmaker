# Kubernetes pods

Browse pods across all namespaces in the current Kubernetes context, read their
logs, and open a shell in a pod. This is a quick inspector for the cluster you
are already pointed at.

## Usage

```sh
mm -o kube/pods
```

Requirements: `kubectl` configured with a current context, and `$EDITOR` for
editing logs. The preset relies on your existing kubeconfig and does not change
contexts or namespaces.

Rows come from `kubectl get pods --all-namespaces`; each row is one pod. The
prompt is prefixed with the current context name.

The main preview follows the pod's logs from the last 100 lines, across all
containers. It is a live stream, so it stays open and updates while the preview
is visible. `<enter>` opens an interactive shell in the pod, trying `bash` and
falling back to `sh`; pods without either shell will fail. `@open` opens the
pod's logs in `$EDITOR` through process substitution.

The preset is read-only: it does not delete, restart, pause, or scale pods.
Use `kubectl` directly for those operations.
