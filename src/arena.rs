//! Structure d'arène générationnelle (*Generational Arena* / *Object Pool*).
//!
//! Ce module fournit une [`Arena`] permettant de stocker des éléments de manière contiguë en mémoire.
//! L'accès aux éléments se fait via un identifiant léger [`Id`] qui associe un index d'emplacement
//! à un numéro de **génération**.
//!
//! ### Avantages de l'indexation générationnelle :
//! - **Accès $O(1)$** direct aux éléments sans recherche.
//! - **Prévention des références pendantes (*Dangling Pointers*) / Problème ABA** : Si un objet est supprimé
//!   puis un autre réinséré au même index, le compteur de génération augmente, rendant les anciens [`Id`] caducs.

use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

/// Identifiant unique permettant de retrouver un élément dans une [`Arena`].
///
/// Un `Id` combine un index de tableau avec un compteur de génération. Il est léger,
/// implémente `Copy`, et peut être fortement typé grâce au paramètre de genre `Tag`.
pub struct Id<Tag> {
    /// Index de l'emplacement (*slot*) dans le vecteur interne de l'arène.
    pub index: usize,
    /// Génération enregistrée lors de la création de cet ID.
    pub generation: u32,
    _phantom: PhantomData<Tag>,
}

impl<Tag> Id<Tag> {
    pub fn zero() -> Self {
        Id {
            index: 0,
            generation: 0,
            _phantom: PhantomData,
        }
    }
}

impl<T> std::fmt::Display for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "idx: {} gen: {}", self.index, self.generation)
    }
}

// Implémentations des traits standards pour Id<T>
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Id {{ index: {}, generation: {} }}",
            self.index, self.generation
        )
    }
}

impl<T> Copy for Id<T> {}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Emplacement interne (*slot*) dans l'arène contenant la valeur et sa génération actuelle.
struct Slot<Data> {
    generation: u32,
    value: Option<Data>,
}

/// Une arène générationnelle permettant une allocation et une réutilisation efficace d'emplacements mémoire.
///
/// # Paramètres de type
/// - `Data`: Le type des données stockées dans l'arène.
/// - `Tag`: Type fantôme permettant de marquer la clé (par défaut égal à `Data`). Cela permet d'éviter de
///   mélanger accidentellement des `Id` issus d'arènes différentes.
///
/// # Exemple
/// ```rust
/// use utils::Arena;
///
/// let mut arena: Arena<String> = Arena::new();
/// let id = arena.insert("Hello World".to_string());
///
/// assert_eq!(arena.get(id), Some(&"Hello World".to_string()));
///
/// arena.remove(id);
/// assert_eq!(arena.get(id), None); // L'ID est maintenant invalide
/// ```
pub struct Arena<Data, Tag = Data> {
    nodes: Vec<Slot<Data>>,
    free_slot: Vec<usize>,
    _phantom: PhantomData<Tag>,
}

impl<Data, Tag> Arena<Data, Tag> {
    pub fn allocated_slots(&self) -> usize {
        self.nodes.len()
    }
    /// Crée une nouvelle arène vide sans allocation initiale.
    pub fn new() -> Self {
        Arena {
            nodes: Vec::new(),
            free_slot: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Crée une arène pré-allouée avec une capacité donnée et pré-remplit la pile d'emplacements libres.
    ///
    /// # Arguments
    /// * `capacity` - Le nombre d'emplacements à réserver d'avance.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut free_slot = Vec::with_capacity(capacity);

        for i in 0..capacity {
            free_slot.push(capacity - 1 - i);
        }

        Self {
            nodes: Vec::with_capacity(capacity),
            free_slot,
            _phantom: PhantomData,
        }
    }

    /// Insère un élément dans l'arène et retourne son [`Id`].
    ///
    /// Si des emplacements ont été libérés auparavant, l'arène recyclera le dernier emplacement
    /// libéré et incrémentera son compteur de génération. Sinon, un nouvel emplacement sera alloué.
    #[must_use]
    pub fn insert(&mut self, data: Data) -> Id<Tag> {
        if let Some(index) = self.free_slot.pop() {
            if index < self.nodes.len() {
                let node = &mut self.nodes[index];
                node.value = Some(data);
                node.generation += 1;

                Id {
                    index,
                    generation: node.generation,
                    _phantom: std::marker::PhantomData,
                }
            } else {
                self.nodes.push(Slot {
                    value: Some(data),
                    generation: 0,
                });

                Id {
                    index: self.nodes.len() - 1,
                    generation: 0,
                    _phantom: std::marker::PhantomData,
                }
            }
        } else {
            self.nodes.push(Slot {
                value: Some(data),
                generation: 0,
            });

            Id {
                index: self.nodes.len() - 1,
                generation: 0,
                _phantom: std::marker::PhantomData,
            }
        }
    }

    /// Récupère une référence immutable vers l'élément associé à l'[`Id`].
    ///
    /// Renvoie `None` si :
    /// - L'index est hors-limites.
    /// - La génération de l'[`Id`] ne correspond pas à celle du slot (élément supprimé ou remplacé).
    /// - L'emplacement est vide.
    #[must_use]
    pub fn get(&self, id: Id<Tag>) -> Option<&Data> {
        if id.index >= self.nodes.len()
            || id.generation != self.nodes[id.index].generation
            || self.nodes[id.index].value.is_none()
        {
            return None;
        }
        self.nodes[id.index].value.as_ref()
    }

    /// Récupère une référence mutable vers l'élément associé à l'[`Id`].
    ///
    /// Renvoie `None` si l'[`Id`] est invalide ou si le slot est vide.
    #[must_use]
    pub fn get_mut(&mut self, id: Id<Tag>) -> Option<&mut Data> {
        if id.index >= self.nodes.len()
            || id.generation != self.nodes[id.index].generation
            || self.nodes[id.index].value.is_none()
        {
            return None;
        }
        self.nodes[id.index].value.as_mut()
    }

    /// Supprime un élément de l'arène à partir de son [`Id`] et le retourne.
    ///
    /// Libère l'emplacement correspondant pour qu'il puisse être recyclé lors d'une future insertion.
    /// Renvoie `None` si l'[`Id`] fourni n'est pas valide ou est obsolète.
    pub fn remove(&mut self, id: Id<Tag>) -> Option<Data> {
        if id.index >= self.nodes.len() || id.generation != self.nodes[id.index].generation {
            return None;
        }
        self.free_slot.push(id.index);
        self.nodes[id.index].value.take()
    }

    /// Retourne un itérateur immutable sur l'ensemble des éléments actifs de l'arène.
    pub fn iter(&self) -> impl Iterator<Item = &Data> {
        self.nodes.iter().filter_map(|slot| slot.value.as_ref())
    }

    /// Retourne un itérateur mutable sur l'ensemble des éléments actifs de l'arène.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Data> {
        self.nodes.iter_mut().filter_map(|slot| slot.value.as_mut())
    }

    /// Retourne un itérateur produisant des paires `(Id<Tag>, &Data)` pour tous les éléments actifs.
    pub fn iter_with_ids(&self) -> impl Iterator<Item = (Id<Tag>, &Data)> {
        self.nodes.iter().enumerate().filter_map(|(index, slot)| {
            slot.value.as_ref().map(|data| {
                (
                    Id {
                        index,
                        generation: slot.generation,
                        _phantom: PhantomData,
                    },
                    data,
                )
            })
        })
    }

    /// Réserve un [`Id`] issu de la pile des slots libres sans nécessairement insérer de valeur à cet instant.
    ///
    /// Utile pour les architectures de *pooling* bas niveau.
    /// Renvoie `None` si aucun slot libre pré-alloué n'est disponible.
    #[must_use]
    pub fn acquire(&mut self) -> Option<Id<Tag>> {
        let index = self.free_slot.pop()?;
        if index < self.nodes.len() {
            Some(Id {
                index,
                generation: self.nodes[index].generation,
                _phantom: PhantomData,
            })
        } else {
            None
        }
    }

    /// Libère manuellement un emplacement d'index associé à un [`Id`] et incrémente sa génération.
    pub fn release_index(&mut self, id: Id<Tag>) {
        if id.index < self.nodes.len() && id.generation == self.nodes[id.index].generation {
            self.nodes[id.index].generation += 1;
            self.free_slot.push(id.index);
        }
    }

    /// Initialise directement un emplacement en fin de vecteur si la capacité maximale n'a pas été atteinte.
    ///
    /// Renvoie `None` si la capacité est atteinte.
    pub fn init_slot(&mut self, value: Data) -> Option<()> {
        if self.nodes.len() >= self.nodes.capacity() {
            return None;
        }
        self.nodes.push(Slot {
            generation: 0,
            value: Some(value),
        });
        Some(())
    }
}

impl<Data, Tag> Default for Arena<Data, Tag> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert() {
        let mut arena: Arena<i32> = Arena::new();
        let id = arena.insert(10);
        let value = arena.get(id).expect("devrait être là");
        assert_eq!(*value, 10);
    }

    #[test]
    fn test_multi_insert() {
        let mut arena: Arena<i32> = Arena::new();
        let id1 = arena.insert(1);
        let id2 = arena.insert(2);
        let id3 = arena.insert(3);

        let value1 = arena.get(id1).unwrap();
        let value2 = arena.get(id2).unwrap();
        let value3 = arena.get(id3).unwrap();

        assert_eq!(*value1, 1);
        assert_eq!(*value2, 2);
        assert_eq!(*value3, 3);
    }

    #[test]
    fn test_remove() {
        let mut arena: Arena<i32> = Arena::new();
        let id = arena.insert(1);
        arena.remove(id);
        assert_eq!(arena.get(id), None);
    }

    #[test]
    fn test_remove_insert() {
        let mut arena: Arena<i32> = Arena::new();
        let id_ancien = arena.insert(1);
        arena.remove(id_ancien);
        let nouv_id = arena.insert(2);
        let value = arena.get(nouv_id).unwrap();

        assert_eq!(*value, 2);
        assert_ne!(
            id_ancien.generation, nouv_id.generation,
            "La génération doit avoir augmenté"
        );
    }

    #[test]
    fn test_ancien_id_invalide_apres_reinsert() {
        let mut arena: Arena<i32> = Arena::new();
        let id_ancien = arena.insert(1);
        arena.remove(id_ancien);
        let _id_nouveau = arena.insert(2);

        assert_eq!(arena.get(id_ancien), None);
    }

    #[test]
    fn test_index_hors_bornes() {
        let arena: Arena<i32> = Arena::new();
        let id_bidon = Id {
            index: 99,
            generation: 0,
            _phantom: std::marker::PhantomData,
        };
        assert_eq!(arena.get(id_bidon), None);
    }

    #[test]
    fn test_remove_avec_generation_perimee() {
        let mut arena: Arena<i32> = Arena::new();
        let id_ancien = arena.insert(1);
        arena.remove(id_ancien);
        let id_nouveau = arena.insert(2);

        let result = arena.remove(id_ancien);

        assert_eq!(result, None);
        assert_eq!(arena.get(id_nouveau), Some(&2));
    }

    #[test]
    fn test_pool_lifecycle_o1() {
        let mut arena: Arena<i32> = Arena::with_capacity(3);
        arena.init_slot(100); // Index 0
        arena.init_slot(200); // Index 1
        arena.init_slot(300); // Index 2

        assert_eq!(arena.nodes.len(), 3, "Les 3 slots doivent être instanciés");

        let id0 = arena.acquire().expect("Slot 0 disponible");
        let id1 = arena.acquire().expect("Slot 1 disponible");
        let id2 = arena.acquire().expect("Slot 2 disponible");

        assert_eq!(id0.index, 0);
        assert_eq!(id1.index, 1);
        assert_eq!(id2.index, 2);

        assert_eq!(*arena.get(id0).unwrap(), 100);
        assert_eq!(*arena.get(id1).unwrap(), 200);
        assert_eq!(*arena.get(id2).unwrap(), 300);

        arena.release_index(id1);

        let id_recycled = arena.acquire().expect("L'index 1 devrait être ré-acquis");
        assert_eq!(
            id_recycled.index, 1,
            "La pile doit redonner l'index récemment libéré"
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct DummyTag;

    #[test]
    fn test_arena_insert_and_get_basic() {
        let mut arena: Arena<i32, DummyTag> = Arena::new();
        let id = arena.insert(42);

        assert_eq!(arena.get(id), Some(&42));
        assert_eq!(id.index, 0);
        assert_eq!(id.generation, 0);
    }

    #[test]
    fn test_arena_dynamic_growth_cas_c() {
        let mut arena: Arena<String, DummyTag> = Arena::new();

        let id1 = arena.insert("A".to_string());
        let id2 = arena.insert("B".to_string());

        assert_eq!(id1.index, 0);
        assert_eq!(id2.index, 1);
        assert_eq!(arena.get(id1), Some(&"A".to_string()));
        assert_eq!(arena.get(id2), Some(&"B".to_string()));
    }

    #[test]
    fn test_arena_hybrid_capacity_cas_b() {
        let mut arena: Arena<f32, DummyTag> = Arena::with_capacity(3);

        let id1 = arena.insert(1.1);
        let id2 = arena.insert(2.2);

        assert_eq!(id1.index, 0);
        assert_eq!(id2.index, 1);
        assert_eq!(arena.nodes.len(), 2);

        let _id3 = arena.insert(3.3);
        let id4 = arena.insert(4.4);

        assert_eq!(id4.index, 3);
        assert_eq!(arena.get(id4), Some(&4.4));
    }

    #[test]
    fn test_arena_recycling_cas_a() {
        let mut arena: Arena<i32, DummyTag> = Arena::with_capacity(2);
        let id1 = arena.insert(10);
        let _id2 = arena.insert(20);

        arena.remove(id1);
        assert_eq!(arena.get(id1), None);

        let id1_recycled = arena.insert(99);
        assert_eq!(id1_recycled.index, 0, "L'index 0 aurait dû être recyclé");
        assert_eq!(
            id1_recycled.generation, 1,
            "La génération aurait dû augmenter à 1"
        );

        assert_eq!(arena.get(id1), None);
        assert_eq!(arena.get(id1_recycled), Some(&99));
    }

    #[test]
    fn test_arena_outdated_generation_security() {
        let mut arena: Arena<i32, DummyTag> = Arena::with_capacity(1);
        let id_original = arena.insert(100);

        arena.remove(id_original);
        let id_nouveau = arena.insert(200);

        let remove_fail = arena.remove(id_original);
        assert!(
            remove_fail.is_none(),
            "Impossible de supprimer avec un ID périmé"
        );

        assert_eq!(arena.get(id_nouveau), Some(&200));
    }

    #[test]
    fn test_arena_get_mut() {
        let mut arena: Arena<i32, DummyTag> = Arena::new();
        let id = arena.insert(5);

        if let Some(val) = arena.get_mut(id) {
            *val += 10;
        }

        assert_eq!(arena.get(id), Some(&15));
    }
}