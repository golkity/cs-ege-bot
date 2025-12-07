#[derive(Clone, Default)]
pub enum DialogueState {
    #[default]
    Start,
    ChoosingSection { kind: SubmissionType },
    ChoosingTopic { kind: SubmissionType, section: String },
    WaitingForContent { kind: SubmissionType, section: String, topic_id: String, topic_title: String },
    AdminPanel,
    AdminWaitingForExportUser,
    AdminWaitingForDeleteUser,
}

#[derive(Clone, PartialEq, Debug)]
pub enum SubmissionType {
    Dz,
    Conspect,
}