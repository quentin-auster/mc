ALTER TABLE context_snapshots
    ADD COLUMN rendered_artifact_id UUID,
    ADD CONSTRAINT context_snapshots_rendered_artifact_run_fk
        FOREIGN KEY (rendered_artifact_id, run_id) REFERENCES artifacts(id, run_id);

ALTER TABLE context_items
    ADD COLUMN run_id UUID,
    ADD COLUMN rendered_artifact_id UUID;

UPDATE context_items AS item
SET run_id = snapshot.run_id
FROM context_snapshots AS snapshot
WHERE snapshot.id = item.context_snapshot_id;

ALTER TABLE context_items
    ALTER COLUMN run_id SET NOT NULL,
    ADD CONSTRAINT context_items_snapshot_run_fk
        FOREIGN KEY (context_snapshot_id, run_id) REFERENCES context_snapshots(id, run_id),
    ADD CONSTRAINT context_items_rendered_artifact_run_fk
        FOREIGN KEY (rendered_artifact_id, run_id) REFERENCES artifacts(id, run_id);
